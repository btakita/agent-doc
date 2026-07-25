//! Reactive editor-attachment liveness (`#live-editor-reactive`, `#s4b-liveness-cell`).
//!
//! S4b lifts the editor-attached authority gate (`authority_for_file`) off the
//! per-decision filesystem lease poll and onto the lazily reactive backbone. Two
//! reactive inputs and one derived read:
//!
//! - `alive: ThreadSafeCellMap<pid, bool>` — process liveness. Set `true` when an
//!   editor process attaches (register / cold-miss lease seed); set `false` by an OS
//!   process-exit **event** (see [`ProcessExitWatcher`]). This is the crash signal the
//!   reactive stream could not observe before S4b: a crashed editor sends no
//!   `deregister`, but its pid death is an OS event that flips this cell.
//! - `registered: ThreadSafeCellMap<(doc, pid), bool>` — per-document editor
//!   attachment, driven by the replica lifecycle (`register`/`reconnect`/`update` →
//!   `true`, `deregister` → `false`).
//! - `is_attached(doc)` — the authority read: *any registered `(doc, pid)` whose
//!   `alive[pid]` is true*. A pure in-memory reactive read, zero filesystem. Because it
//!   reads `alive[pid]` live, one `process_exited(pid)` cascades to every document that
//!   pid owned (whole-editor death recomputes them all closed).
//!
//! ## Why a watcher gate (the CLI-safety rule)
//!
//! Trusting the reactive cache is only safe in a process that will actually observe an
//! editor **crash**. The long-lived project controller installs an OS
//! [`ProcessExitWatcher`]; a short-lived CLI does not. So the registry only seeds its
//! reactive state (via [`EditorAttach::attach`]) when a watcher is installed. A process
//! with no watcher never marks a document `is_tracked`, so its `authority_for_file`
//! consumer always cold-misses to the durable lease — exactly the pre-S4b behavior, and
//! crash-safe because the lease pid-liveness is read fresh each time. The controller,
//! with a watcher, reads pure reactive state on the hot path and consults the lease only
//! on a cold miss (post-recycle recovery).
//!
//! ## Cross-platform
//!
//! This module is OS-agnostic: it holds only reactive state and an injectable
//! [`ProcessExitWatcher`] trait object. The concrete OS watcher (a portable
//! process-liveness poller — `kill(pid, 0)` on Unix/Linux/macOS, a process handle on
//! Windows) lives in the controller layer behind this trait, and a `SimWorld` test
//! installs a fake watcher and drives synthetic exit events. The derived authority is
//! therefore deterministically testable without real process death.

use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use lazily::{Computed, Source, ThreadSafeCellMap, ThreadSafeContext};

/// A source of process-exit **events** (not a poll on the hot path).
///
/// The controller installs a concrete OS watcher; each attached editor pid is
/// [`watch`](ProcessExitWatcher::watch)ed once (an event, learned at attach time), and
/// when the OS reports the process exited — crash **or** clean exit — the watcher calls
/// [`EditorAttach::process_exited`] to flip the `alive` cell. Implementations must be
/// `Send + Sync` (the watcher lives behind the process-global registry).
///
/// A `SimWorld` test installs a fake watcher that merely records watched pids; the test
/// then drives [`EditorAttach::process_exited`] directly to simulate death.
pub trait ProcessExitWatcher: Send + Sync {
    /// Begin watching `pid` for exit. Idempotent: watching an already-watched pid is a
    /// no-op. Called once per pid at attach time.
    fn watch(&self, pid: u32);

    /// Stop watching `pid` (best-effort; default no-op). Called when the last document
    /// owned by `pid` detaches, to release the OS watch resource.
    fn unwatch(&self, _pid: u32) {}
}

/// Reactive editor-attachment registry.
///
/// Cheap to construct; holds a private [`ThreadSafeContext`]. All reactive handles are
/// `Send + Sync`, so a single instance backs the process-global registry
/// ([`editor_attach`]).
pub struct EditorAttach {
    ctx: ThreadSafeContext,
    /// `pid -> alive`. Materialized `true` on attach; flipped `false` by an OS exit
    /// event. Deferral-not-dealloc: a dead pid's key stays present-but-false.
    alive: ThreadSafeCellMap<u32, bool>,
    /// `(doc, pid) -> registered`. Materialized `true` on attach; flipped `false` on an
    /// explicit deregister. A closed pair stays present-but-false (uncounted).
    registered: ThreadSafeCellMap<(String, u32), bool>,
    /// Bumped when a brand-new key is materialized so the derived count observes it.
    epoch: Source<u64>,
    /// Reactive count of distinct currently-attached documents (observability/tests).
    attached_count: Computed<usize>,
    /// The installed OS process-exit watcher, if any. Absent ⇒ this process does not
    /// seed reactive state (CLI-safety rule), so consumers cold-miss to the lease.
    watcher: Mutex<Option<std::sync::Arc<dyn ProcessExitWatcher>>>,
}

impl EditorAttach {
    /// Build an empty registry joined to `scope`'s graph (`#stategraphjoin`).
    ///
    /// Attachment is a process fact — the registry is a process-global singleton and
    /// the OS process-exit watcher is installed on it once at startup — so it joins
    /// the shared [`editor_process_scope`]. Sharing that scope with
    /// [`crate::editor_open_docs`] is the point: "open" and "attached" are now cells
    /// in one graph, so a derivation can span them instead of a caller reading two
    /// islands and combining them by hand.
    pub fn new_in(scope: &agent_doc_state_scope::ProcessScope) -> Self {
        Self::build(scope.ctx().clone())
    }

    /// Standalone registry in a private context — a pure helper for unit tests only.
    /// A long-lived owner must use [`Self::new_in`]; see `#stategraphjoin`.
    pub fn new() -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        Self::build(ThreadSafeContext::new())
    }

    fn build(ctx: ThreadSafeContext) -> Self {
        let epoch = ctx.source(0u64);
        let alive: ThreadSafeCellMap<u32, bool> = ThreadSafeCellMap::new(&ctx);
        let registered: ThreadSafeCellMap<(String, u32), bool> = ThreadSafeCellMap::new(&ctx);
        let attached_count = {
            let registered = registered.clone();
            let alive = alive.clone();
            ctx.computed(move |ctx| {
                // Depend on the membership epoch so a newly-present key forces a
                // recompute that then picks it up in `present_keys`.
                let _ = ctx.get(&epoch);
                let mut docs: BTreeSet<String> = BTreeSet::new();
                for key in registered.present_keys() {
                    let (doc, pid) = key.clone();
                    if registered.observe(ctx, &key).unwrap_or(false)
                        && alive.observe(ctx, &pid).unwrap_or(false)
                    {
                        docs.insert(doc);
                    }
                }
                docs.len()
            })
        };
        Self {
            ctx,
            alive,
            registered,
            epoch,
            attached_count,
            watcher: Mutex::new(None),
        }
    }

    /// Install the OS process-exit watcher. Called once by the controller at startup.
    /// After this, [`attach`](Self::attach) seeds reactive state and requests a watch;
    /// before it (or in a CLI that never installs one), `attach` is a no-op so consumers
    /// keep cold-missing to the durable lease.
    pub fn install_watcher(&self, watcher: std::sync::Arc<dyn ProcessExitWatcher>) {
        *self.watcher.lock() = Some(watcher);
    }

    /// Whether an OS process-exit watcher is installed in this process.
    pub fn has_watcher(&self) -> bool {
        self.watcher.lock().is_some()
    }

    fn bump_epoch(&self) {
        let epoch = self.ctx.get(&self.epoch);
        self.ctx.set(&self.epoch, epoch.wrapping_add(1));
    }

    /// Materialize (if needed) and set a `registered` pair. Never holds the family lock
    /// across the `ctx` write.
    fn set_registered(&self, doc: &str, pid: u32, value: bool) -> bool {
        let key = (doc.to_string(), pid);
        let newly_present = !self.registered.is_present(&key);
        self.registered.set(&self.ctx, key, value);
        newly_present
    }

    /// Set a pid's `alive` cell.
    fn set_alive(&self, pid: u32, value: bool) -> bool {
        let newly_present = !self.alive.is_present(&pid);
        self.alive.set(&self.ctx, pid, value);
        newly_present
    }

    /// Record that editor process `pid` is attached to `doc`, seeding the reactive
    /// authority and requesting an OS exit-watch for `pid`.
    ///
    /// **No-op when no watcher is installed** (CLI-safety rule): without an exit watcher
    /// the reactive `alive` cell could go stale on a crash, so a watcher-less process must
    /// keep reading the durable lease instead of a seeded cache. Idempotent under a
    /// watcher: re-attaching refreshes `registered`/`alive` to true and re-watches (pids
    /// are reused by the OS, so a fresh attach always (re)asserts liveness).
    pub fn attach(&self, doc: &str, pid: u32) {
        let watcher = {
            let guard = self.watcher.lock();
            match guard.as_ref() {
                Some(w) => w.clone(),
                None => return,
            }
        };
        let mut new_key = self.set_registered(doc, pid, true);
        // A fresh attach asserts the pid is live (the register event proved a live editor
        // endpoint), overriding any prior exit mark for a reused pid number.
        new_key |= self.set_alive(pid, true);
        if new_key {
            self.bump_epoch();
        }
        watcher.watch(pid);
    }

    /// Mark every `(doc, *)` pair closed on an explicit editor deregister. The pid's
    /// `alive` cell is left as-is (another document may still be attached to the same
    /// editor pid); the OS watch is released only when no registered document remains for
    /// that pid.
    pub fn detach(&self, doc: &str) {
        let mut freed_pids: Vec<u32> = Vec::new();
        for key in self.registered.present_keys() {
            if key.0 == doc && self.registered.observe(&self.ctx, &key).unwrap_or(false) {
                let pid = key.1;
                self.set_registered(doc, pid, false);
                if !self.pid_has_registered_doc(pid) {
                    freed_pids.push(pid);
                }
            }
        }
        if !freed_pids.is_empty()
            && let Some(watcher) = self.watcher.lock().as_ref()
        {
            for pid in freed_pids {
                watcher.unwatch(pid);
            }
        }
    }

    /// Whether any still-registered document is attached to `pid`.
    fn pid_has_registered_doc(&self, pid: u32) -> bool {
        self.registered
            .present_keys()
            .into_iter()
            .any(|key| key.1 == pid && self.registered.observe(&self.ctx, &key).unwrap_or(false))
    }

    /// OS exit event: `pid` has exited (crash **or** clean exit). Flips the `alive` cell
    /// so every document that pid owned recomputes to detached. Called by the installed
    /// [`ProcessExitWatcher`]; also the `SimWorld` injection point.
    pub fn process_exited(&self, pid: u32) {
        // Only meaningful for a pid we materialized; still safe to set for an unknown pid
        // (is_attached only consults `alive` for registered pids).
        if self.set_alive(pid, false) {
            self.bump_epoch();
        }
    }

    /// Whether `doc` currently has a **live** attached editor: some registered
    /// `(doc, pid)` whose `alive[pid]` is true. Pure reactive read, no filesystem.
    pub fn is_attached(&self, doc: &str) -> bool {
        self.registered.present_keys().into_iter().any(|key| {
            key.0 == doc
                && self.registered.observe(&self.ctx, &key).unwrap_or(false)
                && self.alive.observe(&self.ctx, &key.1).unwrap_or(false)
        })
    }

    /// Whether `doc` has ever been recorded (attached OR detached) in this registry.
    /// Distinguishes a **known** document (read the reactive authority) from a
    /// **never-seen / post-recycle** one (cold-miss to the durable lease). A watcher-less
    /// process never records anything, so this is always false there → always cold-miss.
    pub fn is_tracked(&self, doc: &str) -> bool {
        self.registered
            .present_keys()
            .into_iter()
            .any(|key| key.0 == doc)
    }

    /// Number of distinct documents with a live attached editor — a reactive read of the
    /// derived `attached_count` slot.
    pub fn attached_count(&self) -> usize {
        self.ctx.get(&self.attached_count)
    }

    /// The currently live-attached document paths (unordered, deduplicated).
    pub fn attached_docs(&self) -> Vec<String> {
        let mut docs: BTreeSet<String> = BTreeSet::new();
        for key in self.registered.present_keys() {
            let (doc, pid) = key.clone();
            if self.registered.observe(&self.ctx, &key).unwrap_or(false)
                && self.alive.observe(&self.ctx, &pid).unwrap_or(false)
            {
                docs.insert(doc);
            }
        }
        docs.into_iter().collect()
    }
}

impl Default for EditorAttach {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-global editor-attachment registry. The controller installs an OS watcher
/// on it at startup; the replica lifecycle drives `attach`/`detach`; the authority gate
/// reads `is_attached`/`is_tracked`.
pub fn editor_attach() -> &'static EditorAttach {
    static GLOBAL: OnceLock<EditorAttach> = OnceLock::new();
    GLOBAL.get_or_init(|| EditorAttach::new_in(crate::editor_process_scope()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex as StdMutex;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// A fake OS watcher: records watched pids so a test can assert an attach requested a
    /// watch, and drive `process_exited` synthetically (the `SimWorld` exit-event source).
    #[derive(Default)]
    struct FakeWatcher {
        watched: StdMutex<BTreeSet<u32>>,
        unwatched: StdMutex<BTreeSet<u32>>,
    }
    impl ProcessExitWatcher for FakeWatcher {
        fn watch(&self, pid: u32) {
            self.watched.lock().insert(pid);
        }
        fn unwatch(&self, pid: u32) {
            self.unwatched.lock().insert(pid);
        }
    }

    fn with_watcher() -> (EditorAttach, Arc<FakeWatcher>) {
        let ea = EditorAttach::new();
        let w = Arc::new(FakeWatcher::default());
        ea.install_watcher(w.clone());
        (ea, w)
    }

    #[test]
    fn attach_is_a_noop_without_a_watcher() {
        // CLI-safety rule: no watcher installed ⇒ no reactive state seeded ⇒ the doc is
        // never tracked, so the consumer keeps cold-missing to the durable lease.
        let ea = EditorAttach::new();
        assert!(!ea.has_watcher());
        ea.attach("plan.md", 1234);
        assert!(!ea.is_tracked("plan.md"));
        assert!(!ea.is_attached("plan.md"));
        assert_eq!(ea.attached_count(), 0);
    }

    #[test]
    fn attach_then_reactive_read_is_multi_replica() {
        let (ea, w) = with_watcher();
        ea.attach("plan.md", 1234);
        assert!(ea.is_tracked("plan.md"));
        assert!(ea.is_attached("plan.md"));
        assert_eq!(ea.attached_count(), 1);
        assert!(w.watched.lock().contains(&1234), "attach requested a watch");
    }

    #[test]
    fn process_exit_event_demotes_to_detached_but_stays_tracked() {
        let (ea, _w) = with_watcher();
        ea.attach("plan.md", 1234);
        assert!(ea.is_attached("plan.md"));

        // OS reports the editor process died (crash — no deregister was sent).
        ea.process_exited(1234);
        assert!(!ea.is_attached("plan.md"), "dead pid ⇒ no live editor");
        // Still tracked: the consumer reads the reactive authority (detached) and does
        // NOT re-consult the lease. This is the crash detection the poll used to provide.
        assert!(ea.is_tracked("plan.md"));
        assert_eq!(ea.attached_count(), 0);
    }

    #[test]
    fn whole_editor_death_cascades_to_every_doc_that_pid_owned() {
        let (ea, _w) = with_watcher();
        ea.attach("a.md", 100);
        ea.attach("b.md", 100);
        ea.attach("c.md", 200); // a different editor process
        assert_eq!(ea.attached_count(), 3);

        // One exit event for pid 100 recomputes both a.md and b.md to detached; c.md
        // (owned by pid 200) is untouched.
        ea.process_exited(100);
        assert!(!ea.is_attached("a.md"));
        assert!(!ea.is_attached("b.md"));
        assert!(ea.is_attached("c.md"));
        assert_eq!(ea.attached_count(), 1);
    }

    #[test]
    fn explicit_deregister_detaches_and_frees_the_pid_watch() {
        let (ea, w) = with_watcher();
        ea.attach("plan.md", 1234);
        assert!(ea.is_attached("plan.md"));

        ea.detach("plan.md");
        assert!(!ea.is_attached("plan.md"), "graceful close ⇒ detached");
        assert!(ea.is_tracked("plan.md"), "known-closed, not cold-miss");
        assert!(
            w.unwatched.lock().contains(&1234),
            "the last doc for the pid detached ⇒ release the OS watch"
        );
    }

    #[test]
    fn deregister_keeps_watch_while_another_doc_shares_the_pid() {
        let (ea, w) = with_watcher();
        ea.attach("a.md", 100);
        ea.attach("b.md", 100);

        ea.detach("a.md");
        assert!(!ea.is_attached("a.md"));
        assert!(
            ea.is_attached("b.md"),
            "b.md still attached to the live pid"
        );
        assert!(
            !w.unwatched.lock().contains(&100),
            "pid still owns b.md ⇒ watch not released yet"
        );

        ea.detach("b.md");
        assert!(
            w.unwatched.lock().contains(&100),
            "last doc for the pid gone ⇒ watch released"
        );
    }

    #[test]
    fn reattach_after_exit_reasserts_liveness_for_a_reused_pid() {
        let (ea, _w) = with_watcher();
        ea.attach("plan.md", 1234);
        ea.process_exited(1234);
        assert!(!ea.is_attached("plan.md"));

        // A new editor process happens to get the same pid number and re-registers.
        ea.attach("plan.md", 1234);
        assert!(
            ea.is_attached("plan.md"),
            "a fresh attach reasserts alive for the reused pid"
        );
    }

    #[test]
    fn is_tracked_distinguishes_never_seen_from_known() {
        let (ea, _w) = with_watcher();
        assert!(!ea.is_tracked("never.md"));
        ea.attach("plan.md", 1);
        assert!(ea.is_tracked("plan.md"));
        ea.detach("plan.md");
        assert!(ea.is_tracked("plan.md"), "known-closed is still tracked");
    }

    /// Deterministic SimWorld: fold a scripted event stream through the reactive registry
    /// and a pure reference model, asserting the derived attachment matches at every step.
    #[test]
    fn reactive_attachment_matches_reference_model_across_events() {
        #[derive(Clone, Copy, Debug)]
        enum Ev<'a> {
            Attach(&'a str, u32),
            Detach(&'a str),
            Exit(u32),
        }
        use Ev::*;

        // Reference model: registered pairs + a per-pid alive flag.
        #[derive(Default)]
        struct Model {
            registered: BTreeMap<(String, u32), bool>,
            alive: BTreeMap<u32, bool>,
        }
        impl Model {
            fn attach(&mut self, doc: &str, pid: u32) {
                self.registered.insert((doc.to_string(), pid), true);
                self.alive.insert(pid, true);
            }
            fn detach(&mut self, doc: &str) {
                for ((d, _), v) in self.registered.iter_mut() {
                    if d == doc {
                        *v = false;
                    }
                }
            }
            fn exit(&mut self, pid: u32) {
                self.alive.insert(pid, false);
            }
            fn is_attached(&self, doc: &str) -> bool {
                self.registered.iter().any(|((d, pid), reg)| {
                    d == doc && *reg && *self.alive.get(pid).unwrap_or(&false)
                })
            }
            fn attached_count(&self) -> usize {
                let docs: BTreeSet<&String> = self
                    .registered
                    .iter()
                    .filter(|((_, pid), reg)| **reg && *self.alive.get(pid).unwrap_or(&false))
                    .map(|((d, _), _)| d)
                    .collect();
                docs.len()
            }
        }

        let script = [
            Attach("plan.md", 100),
            Attach("notes.md", 100),
            Attach("src.md", 200),
            Exit(100),
            Attach("plan.md", 300), // reopened under a new editor process
            Detach("src.md"),
            Attach("src.md", 200), // reopen a previously-closed doc, pid still alive
            Exit(300),
        ];

        let (ea, _w) = with_watcher();
        let mut model = Model::default();
        let docs = ["plan.md", "notes.md", "src.md"];

        for (step, ev) in script.into_iter().enumerate() {
            match ev {
                Attach(doc, pid) => {
                    ea.attach(doc, pid);
                    model.attach(doc, pid);
                }
                Detach(doc) => {
                    ea.detach(doc);
                    model.detach(doc);
                }
                Exit(pid) => {
                    ea.process_exited(pid);
                    model.exit(pid);
                }
            }
            for doc in docs {
                assert_eq!(
                    ea.is_attached(doc),
                    model.is_attached(doc),
                    "is_attached({doc}) diverged at step {step} after {ev:?}",
                );
            }
            assert_eq!(
                ea.attached_count(),
                model.attached_count(),
                "attached_count diverged at step {step} after {ev:?}",
            );
        }
    }
}

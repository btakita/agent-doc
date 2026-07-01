//! # Module: supervisor::in_process
//!
//! ## Spec (`#pcpc2` — in-process supervisor adapter)
//! Hosts the harness child PTY/stdin/restart/heartbeat as a *controller-managed
//! task* instead of a separate OS process. Today `agent-doc start` runs the
//! supervisor as its own foreground process in a tmux pane (see `start.rs` +
//! `supervisor/ipc.rs`); per `specs/08b` §"In-process actors" the target is one
//! supervisor adapter per managed harness child, living inside the controller
//! process so the session actor (`#pcpc1`) can serialize writes against it.
//!
//! This module is the adapter itself: the lifecycle state machine, restart
//! policy, stdin injection, heartbeat, and reattach logic,
//! decoupled from any specific child via the [`SupervisedChild`] trait. A real
//! PTY child plugs in through [`PtySupervisedChild`] (a thin wrapper over
//! `supervisor::pty`); SimWorld tests plug in a deterministic fake child so the
//! adapter logic is provable without a live harness or wall-clock timing.
//!
//! ## Agentic Contracts
//! - The adapter owns exactly one live child at a time; `generation()` counts
//!   spawns so a reattach (no new child) is distinguishable from a restart.
//! - `tick_at(now)` is the single controller-driven step: it advances the
//!   heartbeat and, on child exit, consults [`CrashPolicy`] for the next action.
//! - Heartbeat loss is observed externally (the controller compares the last
//!   beat it saw against the current beat). `reattach()` rebinds to a still-live
//!   child WITHOUT restarting it — a stale heartbeat is not proof the child died.
//! - The external Unix-socket boundary (`IpcMethod`) is preserved: CLI/editor
//!   callers still talk to the controller, which forwards to these methods. The
//!   live `agent-doc start` cutover is gated in `#pcpc5` (08b gate ladder), so
//!   this phase lands without touching the running out-of-process supervisor.
//!
//! ## Evals
//! - `in_process_supervisor_heartbeat_advances_on_tick`
//! - `in_process_supervisor_heartbeat_lost_reattaches_live_child_without_restart`
//! - `in_process_supervisor_restarts_after_transient_exit`
//! - `in_process_supervisor_halts_after_flap_storm`
//! - `in_process_supervisor_prompts_on_clean_exit`
//! - `in_process_supervisor_inject_reaches_child_stdin`
//! - `in_process_supervisor_explicit_restart_resets_policy`

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use anyhow::Result;

use agent_doc_supervisor::crash_policy::{CrashPolicy, RestartAction, SupervisorState};

/// A child process the supervisor owns. Abstracted so the adapter logic is
/// testable with a deterministic fake while the production path wraps a real
/// PTY child (`PtySupervisedChild`).
pub trait SupervisedChild: Send {
    /// OS process id, if the child is spawned and known.
    fn pid(&self) -> Option<u32>;
    /// Whether the child is still running (has not yet been reaped).
    fn is_alive(&self) -> bool;
    /// Non-blocking exit check: `Some(code)` once the child has exited, else
    /// `None`. Must be idempotent after the first `Some`.
    fn try_exit_code(&mut self) -> Option<i32>;
    /// Forward operator/agent input to the child's stdin.
    fn write_stdin(&mut self, bytes: &[u8]) -> Result<()>;
    /// Best-effort terminate.
    fn kill(&mut self) -> Result<()>;
}

/// Spawns a fresh child. `with_continue` mirrors [`RestartAction::RestartAfter`]
/// — a resumed (`--continue`) vs fresh start.
pub type ChildFactory = Box<dyn FnMut(bool) -> Result<Box<dyn SupervisedChild>> + Send>;

/// Outcome of a single [`InProcessSupervisor::tick_at`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickOutcome {
    /// Child still running; heartbeat advanced.
    Running,
    /// Child exited and was restarted (new generation spawned).
    Restarted { generation: u64, exit_code: i32 },
    /// Child exited cleanly; caller should prompt the operator (restart/quit).
    PromptOperator { exit_code: i32 },
    /// Child exited and the supervisor is now halted (flap storm).
    Halted { exit_code: i32 },
    /// A restart was due but spawning the replacement child failed.
    RestartFailed { exit_code: i32, error: String },
    /// The supervisor was stopped; no child is running.
    Stopped,
}

/// Result of a reattach attempt after observed heartbeat loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReattachOutcome {
    /// The live child is still present (same pid); monitoring resumed without a
    /// restart. A stale heartbeat alone is never grounds to kill the child.
    Reattached { pid: Option<u32> },
    /// No live child to reattach to (it exited); the next tick will restart it
    /// per policy.
    ChildGone,
    /// The supervisor was explicitly stopped; nothing to reattach.
    Stopped,
}

/// The in-process supervisor adapter: one per managed harness child, driven by
/// the controller's task loop.
pub struct InProcessSupervisor {
    factory: ChildFactory,
    child: Option<Box<dyn SupervisedChild>>,
    policy: CrashPolicy,
    heartbeat: Arc<AtomicU64>,
    generation: u64,
    stopped: bool,
}

impl InProcessSupervisor {
    /// Spawn the initial (fresh) child and build the adapter. The controller
    /// then drives it with `tick_at` and forwards external IPC to the control
    /// methods.
    pub fn spawn(mut factory: ChildFactory) -> Result<Self> {
        let child = factory(false)?;
        Ok(Self {
            factory,
            child: Some(child),
            policy: CrashPolicy::new(),
            heartbeat: Arc::new(AtomicU64::new(0)),
            generation: 1,
            stopped: false,
        })
    }

    /// Adopt a pre-spawned child instead of spawning through a factory. Used by
    /// the `start.rs` in-process host loop (`#pcpc5e1` authority rung): `start.rs`
    /// spawns the `PtySession` and sets up its reader/writer/resize threads, then
    /// hands the child here so the adapter owns exit reaping, heartbeat, and the
    /// crash-policy decision for that generation. Respawn stays with the
    /// `start.rs` outer restart loop (which rebuilds the I/O plumbing), so the
    /// factory is wired to fail if the policy ever asks for an in-adapter restart
    /// — a transient exit then surfaces as `RestartFailed { exit_code, .. }`,
    /// carrying the real code back to the host loop without spawning a child the
    /// host loop is not tracking.
    pub fn adopt(child: Box<dyn SupervisedChild>) -> Self {
        Self {
            factory: Box::new(|_with_continue| {
                anyhow::bail!(
                    "in-process supervisor adopted a pre-spawned child; respawn is owned by the start.rs supervisor restart loop (#pcpc5e1)"
                )
            }),
            child: Some(child),
            policy: CrashPolicy::new(),
            heartbeat: Arc::new(AtomicU64::new(0)),
            generation: 1,
            stopped: false,
        }
    }

    /// Kill the live child WITHOUT halting the supervisor, so the next
    /// [`tick_at`](Self::tick_at) observes the exit and reports its code. The
    /// `start.rs` in-process host loop calls this to honor an external
    /// stop/restart/route-complete request — the out-of-process host path kills
    /// via `libc::kill` by PID; this is the in-process equivalent.
    pub fn kill_child(&mut self) -> Result<()> {
        match self.child.as_mut() {
            Some(child) => child.kill(),
            None => Ok(()),
        }
    }

    /// Shared handle to the heartbeat counter. The controller snapshots this and
    /// compares against a later read to detect heartbeat loss without reaching
    /// into the adapter.
    pub fn heartbeat_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.heartbeat)
    }

    /// Current heartbeat value (monotonic; bumped once per `tick_at`).
    pub fn heartbeat(&self) -> u64 {
        self.heartbeat.load(Ordering::SeqCst)
    }

    /// Number of children spawned so far (initial spawn = 1). Unchanged by a
    /// reattach; incremented by every restart.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Current health state from the crash policy.
    pub fn state(&self) -> SupervisorState {
        self.policy.state
    }

    /// Live child pid, if any.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.pid())
    }

    /// Whether the supervisor has been explicitly stopped.
    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// One controller-driven step. Advances the heartbeat, then — if the child
    /// has exited — applies the crash policy (restart / prompt / halt). With an
    /// explicit `now` for deterministic testing.
    pub fn tick_at(&mut self, now: Instant) -> TickOutcome {
        if self.stopped {
            return TickOutcome::Stopped;
        }
        // The heartbeat advances every tick the supervisor task runs — its
        // staleness is what tells the controller the task itself wedged, which
        // is distinct from the child exiting (caught below).
        self.heartbeat.fetch_add(1, Ordering::SeqCst);

        let exit_code = match self.child.as_mut() {
            Some(child) => child.try_exit_code(),
            None => None,
        };
        let Some(exit_code) = exit_code else {
            return TickOutcome::Running;
        };

        // Child exited: drop the dead handle and consult policy.
        self.child = None;
        match self.policy.on_exit_at(exit_code, now) {
            RestartAction::Halt => {
                self.stopped = true;
                TickOutcome::Halted { exit_code }
            }
            RestartAction::PromptUser => TickOutcome::PromptOperator { exit_code },
            RestartAction::RestartAfter { with_continue, .. } => {
                match (self.factory)(with_continue) {
                    Ok(child) => {
                        self.child = Some(child);
                        self.generation += 1;
                        TickOutcome::Restarted {
                            generation: self.generation,
                            exit_code,
                        }
                    }
                    Err(e) => {
                        self.stopped = true;
                        TickOutcome::RestartFailed {
                            exit_code,
                            error: e.to_string(),
                        }
                    }
                }
            }
        }
    }

    /// Convenience wrapper using the real clock.
    pub fn tick(&mut self) -> TickOutcome {
        self.tick_at(Instant::now())
    }

    /// Reattach after the controller observed a stale heartbeat. If the child is
    /// still alive we simply resume — a wedged or slow task loop must not cost
    /// the live child a restart. Only a genuinely-exited child reports
    /// `ChildGone`, letting the next `tick_at` restart it under policy.
    pub fn reattach(&mut self) -> ReattachOutcome {
        if self.stopped {
            return ReattachOutcome::Stopped;
        }
        match self.child.as_ref() {
            Some(child) if child.is_alive() => ReattachOutcome::Reattached { pid: child.pid() },
            _ => ReattachOutcome::ChildGone,
        }
    }

    /// Forward bytes to the child's stdin (the in-process equivalent of the
    /// `Inject`/`Clear`/submit IPC methods).
    pub fn inject(&mut self, bytes: &[u8]) -> Result<()> {
        match self.child.as_mut() {
            Some(child) => child.write_stdin(bytes),
            None => anyhow::bail!("no live child to inject into"),
        }
    }

    /// Explicit operator restart (the `Restart` IPC method): kill the current
    /// child, spawn a fresh one, and reset the crash policy so an operator
    /// restart clears a Degraded/Halted state.
    pub fn restart(&mut self, with_continue: bool) -> Result<()> {
        if let Some(mut child) = self.child.take()
            && let Err(e) = child.kill()
        {
            eprintln!("[supervisor] kill before restart failed: {e}");
        }
        let child = (self.factory)(with_continue)?;
        self.child = Some(child);
        self.generation += 1;
        self.policy.reset();
        self.stopped = false;
        Ok(())
    }

    /// Stop the supervisor (the `Stop` IPC method): kill the child and refuse
    /// further restarts until a new adapter/`restart` is created.
    pub fn stop(&mut self) -> Result<()> {
        self.stopped = true;
        if let Some(mut child) = self.child.take() {
            child.kill()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Production child: owns the `portable-pty` child handle taken from a
// `PtySession` via `PtySession::take_child` (`#pcpc5e1` authority rung).
//
// `portable_pty::Child` exposes a non-blocking `try_wait`, so exit reaping needs
// neither a reaper thread nor a channel — `try_exit_code` calls `try_wait`
// directly and caches the first exit. The owning `PtySession` keeps the master
// (reader/writer/resize handles) so `start.rs` retains its PTY-output/prompt
// plumbing while this adapter owns child lifecycle. This is the sole production
// host for the harness child in `agent-doc start` (08b cutover complete).
// ---------------------------------------------------------------------------

use std::io::Write;

use portable_pty::Child;

/// Real PTY-backed [`SupervisedChild`] owning the `portable-pty` child handle.
pub struct PtySupervisedChild {
    pid: Option<u32>,
    /// Stdin writer, when the adapter owns stdin injection. `None` in
    /// monitor-only hosting (the `start.rs` writer thread already owns the pty
    /// writer for keyboard/IPC inject; the adapter then only reaps + kills).
    writer: Option<Box<dyn Write + Send>>,
    child: Box<dyn Child + Send + Sync>,
    exit_code: Option<i32>,
}

impl PtySupervisedChild {
    /// Adopt the child handle taken from a `PtySession`. `writer` is the pty
    /// stdin writer when the adapter owns injection; pass `None` for
    /// monitor-only hosting where `start.rs`'s writer thread keeps stdin.
    pub fn new(
        child: Box<dyn Child + Send + Sync>,
        pid: Option<u32>,
        writer: Option<Box<dyn Write + Send>>,
    ) -> Self {
        Self {
            pid,
            writer,
            child,
            exit_code: None,
        }
    }

    /// Monitor-only constructor: the adapter reaps + kills the child while
    /// `start.rs` keeps the pty writer for keyboard/IPC stdin injection.
    pub fn monitor(child: Box<dyn Child + Send + Sync>, pid: Option<u32>) -> Self {
        Self::new(child, pid, None)
    }

    /// Normalize a `portable_pty::ExitStatus` to the `i32` exit code the crash
    /// policy classifies. `portable-pty` already folds signal termination into a
    /// non-zero code, so a non-success status maps to its `exit_code()` (or `1`
    /// if that is somehow zero).
    fn status_code(status: &portable_pty::ExitStatus) -> i32 {
        let code = status.exit_code() as i32;
        if !status.success() && code == 0 {
            1
        } else {
            code
        }
    }
}

impl SupervisedChild for PtySupervisedChild {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn is_alive(&self) -> bool {
        self.exit_code.is_none()
    }

    fn try_exit_code(&mut self) -> Option<i32> {
        if let Some(code) = self.exit_code {
            return Some(code);
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let code = Self::status_code(&status);
                self.exit_code = Some(code);
                Some(code)
            }
            Ok(None) => None,
            Err(e) => {
                // A reaping error means we can no longer observe the child;
                // surface it (never swallow) and treat the child as exited so the
                // host loop does not spin forever on a dead handle.
                eprintln!("[supervisor::in_process] try_wait failed: {e}");
                self.exit_code = Some(1);
                Some(1)
            }
        }
    }

    fn write_stdin(&mut self, bytes: &[u8]) -> Result<()> {
        match self.writer.as_mut() {
            Some(writer) => {
                writer.write_all(bytes)?;
                writer.flush()?;
                Ok(())
            }
            None => anyhow::bail!(
                "in-process child is monitor-only; stdin is owned by the start.rs writer thread"
            ),
        }
    }

    fn kill(&mut self) -> Result<()> {
        self.child.kill().map_err(anyhow::Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Deterministic fake child. Exit is operator-controlled via a shared cell
    /// so SimWorld tests drive lifecycle transitions without real processes.
    struct FakeChild {
        pid: u32,
        exit: Arc<Mutex<Option<i32>>>,
        stdin: Arc<Mutex<Vec<u8>>>,
    }

    impl SupervisedChild for FakeChild {
        fn pid(&self) -> Option<u32> {
            Some(self.pid)
        }
        fn is_alive(&self) -> bool {
            self.exit.lock().unwrap().is_none()
        }
        fn try_exit_code(&mut self) -> Option<i32> {
            *self.exit.lock().unwrap()
        }
        fn write_stdin(&mut self, bytes: &[u8]) -> Result<()> {
            self.stdin.lock().unwrap().extend_from_slice(bytes);
            Ok(())
        }
        fn kill(&mut self) -> Result<()> {
            *self.exit.lock().unwrap() = Some(137);
            Ok(())
        }
    }

    /// Shared handles to the most-recently-spawned fake child, so a test can
    /// flip its exit cell after the adapter spawned it.
    #[derive(Clone, Default)]
    struct ChildController {
        exit: Arc<Mutex<Option<i32>>>,
        stdin: Arc<Mutex<Vec<u8>>>,
        spawns: Arc<AtomicU64>,
    }

    fn fake_factory(ctrl: ChildController) -> ChildFactory {
        Box::new(move |_with_continue| {
            // A freshly spawned child starts alive: clear the shared exit cell
            // the test mutates to drive the live child's lifecycle.
            *ctrl.exit.lock().unwrap() = None;
            let n = ctrl.spawns.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(Box::new(FakeChild {
                pid: 1000 + n as u32,
                exit: Arc::clone(&ctrl.exit),
                stdin: Arc::clone(&ctrl.stdin),
            }) as Box<dyn SupervisedChild>)
        })
    }

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn in_process_supervisor_heartbeat_advances_on_tick() {
        let ctrl = ChildController::default();
        let mut sup = InProcessSupervisor::spawn(fake_factory(ctrl)).unwrap();
        assert_eq!(sup.heartbeat(), 0);
        let now = t0();
        for expected in 1..=5 {
            assert_eq!(sup.tick_at(now), TickOutcome::Running);
            assert_eq!(sup.heartbeat(), expected);
        }
        // The shared handle observes the same beat — this is what the controller
        // polls to detect a wedged task.
        assert_eq!(sup.heartbeat_handle().load(Ordering::SeqCst), 5);
    }

    #[test]
    fn in_process_supervisor_heartbeat_lost_reattaches_live_child_without_restart() {
        let ctrl = ChildController::default();
        let mut sup = InProcessSupervisor::spawn(fake_factory(ctrl)).unwrap();
        let now = t0();
        sup.tick_at(now);

        let before = sup.heartbeat();
        let gen_before = sup.generation();
        let pid_before = sup.pid();

        // Simulate observed heartbeat loss: the controller saw `before`, then a
        // gap with no advance. heartbeat_lost flags it...
        assert!(agent_doc_supervisor::heartbeat::heartbeat_lost(
            before,
            sup.heartbeat(),
            1
        ));
        // ...but the child is alive, so reattach resumes WITHOUT a restart.
        assert_eq!(
            sup.reattach(),
            ReattachOutcome::Reattached { pid: pid_before }
        );
        assert_eq!(sup.generation(), gen_before, "reattach must not restart");
        assert_eq!(sup.pid(), pid_before, "same child after reattach");
    }

    #[test]
    fn in_process_supervisor_restarts_after_transient_exit() {
        let ctrl = ChildController::default();
        let mut sup = InProcessSupervisor::spawn(fake_factory(ctrl.clone())).unwrap();
        let now = t0();
        assert_eq!(sup.tick_at(now), TickOutcome::Running);
        let pid_before = sup.pid();

        // Child crashes with a non-zero (transient) code.
        *ctrl.exit.lock().unwrap() = Some(1);
        let outcome = sup.tick_at(now);
        assert_eq!(
            outcome,
            TickOutcome::Restarted {
                generation: 2,
                exit_code: 1
            }
        );
        assert_eq!(sup.state(), SupervisorState::Healthy);
        assert!(sup.pid().is_some());
        assert_ne!(sup.pid(), pid_before, "a fresh child after restart");
        // The replacement child is alive again.
        assert_eq!(sup.tick_at(now), TickOutcome::Running);
    }

    #[test]
    fn in_process_supervisor_halts_after_flap_storm() {
        let ctrl = ChildController::default();
        let mut sup = InProcessSupervisor::spawn(fake_factory(ctrl.clone())).unwrap();
        let now = t0();

        // Drive repeated rapid non-zero exits until the policy halts. Each tick:
        // mark exit, tick (restart or halt), then the new child is alive.
        let mut halted = false;
        for _ in 0..12 {
            *ctrl.exit.lock().unwrap() = Some(1);
            match sup.tick_at(now) {
                TickOutcome::Restarted { .. } => {}
                TickOutcome::Halted { exit_code } => {
                    assert_eq!(exit_code, 1);
                    halted = true;
                    break;
                }
                other => panic!("unexpected outcome during flap storm: {other:?}"),
            }
        }
        assert!(halted, "rapid flapping must eventually halt the supervisor");
        assert_eq!(sup.state(), SupervisorState::Halted);
        assert!(sup.is_stopped());
        // A halted supervisor no longer ticks a child.
        assert_eq!(sup.tick_at(now), TickOutcome::Stopped);
    }

    #[test]
    fn in_process_supervisor_prompts_on_clean_exit() {
        let ctrl = ChildController::default();
        let mut sup = InProcessSupervisor::spawn(fake_factory(ctrl.clone())).unwrap();
        let now = t0();
        sup.tick_at(now);
        *ctrl.exit.lock().unwrap() = Some(0);
        assert_eq!(
            sup.tick_at(now),
            TickOutcome::PromptOperator { exit_code: 0 }
        );
        assert_eq!(sup.state(), SupervisorState::Healthy);
    }

    #[test]
    fn in_process_supervisor_inject_reaches_child_stdin() {
        let ctrl = ChildController::default();
        let mut sup = InProcessSupervisor::spawn(fake_factory(ctrl.clone())).unwrap();
        sup.inject(b"do [#x]\n").unwrap();
        assert_eq!(&*ctrl.stdin.lock().unwrap(), b"do [#x]\n");
    }

    #[test]
    fn in_process_supervisor_explicit_restart_resets_policy() {
        let ctrl = ChildController::default();
        let mut sup = InProcessSupervisor::spawn(fake_factory(ctrl.clone())).unwrap();
        let now = t0();

        // Push into Degraded via a couple of flaps (not enough to halt).
        for _ in 0..2 {
            *ctrl.exit.lock().unwrap() = Some(1);
            let _ = sup.tick_at(now);
        }

        // Explicit operator restart clears policy state and spawns fresh.
        let gen_before = sup.generation();
        sup.restart(false).unwrap();
        assert_eq!(sup.generation(), gen_before + 1);
        assert_eq!(sup.state(), SupervisorState::Healthy);
        assert!(!sup.is_stopped());

        // stop() refuses further child work.
        sup.stop().unwrap();
        assert!(sup.is_stopped());
        assert_eq!(sup.tick_at(now), TickOutcome::Stopped);
    }

    // ----- Production PtySupervisedChild coverage (#pcpc5e1 authority rung) -----
    //
    // These drive the real `portable-pty` child through `PtySession::take_child`
    // + `PtySupervisedChild`, proving the non-blocking `try_wait` reaper and the
    // adapter `adopt` host loop the deferred `#pcpc5` stub left untested.

    use super::super::pty::{PtySession, PtySpawnConfig};
    use portable_pty::PtySize;
    use std::collections::HashMap;
    use std::time::Duration;

    fn sh_session(script: &str) -> PtySession {
        let mut env = HashMap::new();
        if let Ok(path) = std::env::var("PATH") {
            env.insert("PATH".to_string(), path);
        }
        let cfg = PtySpawnConfig {
            program: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            cwd: std::env::temp_dir(),
            env,
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        };
        PtySession::spawn(cfg).expect("spawn /bin/sh session")
    }

    /// Poll a child's exit code with a bounded wall-clock budget so a hung child
    /// fails the test instead of hanging the suite.
    fn poll_exit(child: &mut dyn SupervisedChild) -> i32 {
        for _ in 0..200 {
            if let Some(code) = child.try_exit_code() {
                return code;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("child did not exit within budget");
    }

    #[test]
    fn pty_supervised_child_reaps_real_exit_code() {
        let mut session = sh_session("exit 7");
        let pid = session.process_id();
        let child = session.take_child().expect("take child");
        let mut sup_child = PtySupervisedChild::monitor(child, pid);
        assert!(sup_child.is_alive());
        assert_eq!(poll_exit(&mut sup_child), 7);
        // Idempotent after first Some.
        assert_eq!(sup_child.try_exit_code(), Some(7));
        assert!(!sup_child.is_alive());
        // Dropping the session (after take_child) only closes the master; the
        // child handle is owned by the adapter and already reaped.
        drop(session);
    }

    #[test]
    fn pty_supervised_child_kill_terminates_live_child() {
        // Sleep long enough that the child is unambiguously alive when killed.
        let mut session = sh_session("sleep 30");
        let pid = session.process_id();
        let child = session.take_child().expect("take child");
        let mut sup_child = PtySupervisedChild::monitor(child, pid);
        assert_eq!(sup_child.try_exit_code(), None, "child should be alive");
        sup_child.kill().expect("kill live child");
        // A killed child reaps to a non-zero code (signal folded by portable-pty).
        assert_ne!(poll_exit(&mut sup_child), 0);
    }

    #[test]
    fn pty_supervised_child_monitor_rejects_stdin() {
        let mut session = sh_session("exit 0");
        let pid = session.process_id();
        let child = session.take_child().expect("take child");
        let mut sup_child = PtySupervisedChild::monitor(child, pid);
        // Monitor-only hosting: stdin stays with start.rs's writer thread, so the
        // adapter must refuse (never silently drop) an inject.
        assert!(sup_child.write_stdin(b"x").is_err());
        let _ = poll_exit(&mut sup_child);
    }

    #[test]
    fn adopt_hosts_real_child_through_tick_loop() {
        let mut session = sh_session("exit 0");
        let pid = session.process_id();
        let child = session.take_child().expect("take child");
        let mut sup = InProcessSupervisor::adopt(Box::new(PtySupervisedChild::monitor(child, pid)));
        assert_eq!(sup.pid(), pid);

        // Drive the adapter tick loop the way start.rs's in-process host loop
        // does, until the child exits. A clean exit prompts the operator.
        let mut outcome = None;
        for _ in 0..200 {
            match sup.tick() {
                TickOutcome::Running => std::thread::sleep(Duration::from_millis(10)),
                other => {
                    outcome = Some(other);
                    break;
                }
            }
        }
        assert_eq!(outcome, Some(TickOutcome::PromptOperator { exit_code: 0 }));
        assert!(sup.heartbeat() >= 1, "tick loop advanced the heartbeat");
    }

    #[test]
    fn adopt_transient_exit_surfaces_code_without_respawn() {
        // A non-zero (transient) exit makes the crash policy ask for a restart;
        // the adopted factory refuses (respawn is the start.rs loop's job), so the
        // real exit code surfaces as RestartFailed instead of spawning an
        // untracked child.
        let mut session = sh_session("exit 3");
        let pid = session.process_id();
        let child = session.take_child().expect("take child");
        let mut sup = InProcessSupervisor::adopt(Box::new(PtySupervisedChild::monitor(child, pid)));

        let mut outcome = None;
        for _ in 0..200 {
            match sup.tick() {
                TickOutcome::Running => std::thread::sleep(Duration::from_millis(10)),
                other => {
                    outcome = Some(other);
                    break;
                }
            }
        }
        match outcome {
            Some(TickOutcome::RestartFailed { exit_code, .. }) => assert_eq!(exit_code, 3),
            other => panic!("expected RestartFailed carrying exit 3, got {other:?}"),
        }
        assert_eq!(sup.generation(), 1, "no respawn happened in-adapter");
    }

    #[test]
    fn kill_child_lets_tick_report_exit_without_halt() {
        let mut session = sh_session("sleep 30");
        let pid = session.process_id();
        let child = session.take_child().expect("take child");
        let mut sup = InProcessSupervisor::adopt(Box::new(PtySupervisedChild::monitor(child, pid)));

        assert_eq!(sup.tick(), TickOutcome::Running);
        // External stop/restart request: kill the child, keep the supervisor live.
        sup.kill_child().expect("kill child");
        assert!(!sup.is_stopped(), "kill_child must not halt the supervisor");

        let mut exited = false;
        for _ in 0..200 {
            match sup.tick() {
                TickOutcome::Running => std::thread::sleep(Duration::from_millis(10)),
                // A killed child reaps non-zero → policy asks restart → adopted
                // factory refuses → RestartFailed carries the code back.
                TickOutcome::RestartFailed { .. } | TickOutcome::Halted { .. } => {
                    exited = true;
                    break;
                }
                other => panic!("unexpected outcome after kill_child: {other:?}"),
            }
        }
        assert!(exited, "tick loop observed the killed child's exit");
    }
}

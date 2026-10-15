//! `#reexecptybridge` (GH #270): keep a preserved harness child's PTY serviced
//! across a supervisor `execve` hot-reload until the full I/O plumbing adopts it.
//!
//! After `execve`, the replacement image must re-run start admission (registry
//! publish, session re-entry, controller and actor bookkeeping) before it reaches
//! [`crate::pty::PtySession::adopt`] and spawns the reader/writer threads. Those
//! steps can wait on `.agent-doc/state.db` in 30 s `busy_timeout` steps when a
//! batch of supervisors recycles at once. Nothing serviced the inherited master
//! in that window, so every recycled pane froze: no keystrokes reached the child
//! and its output (including a streaming mid-turn response) sat unread.
//!
//! The bridge is a bounded, self-contained pump that starts before any step that
//! can touch `state.db`. It owns no supervisor state and takes no locks shared
//! with start admission: it forwards filtered child output to the pane and pane
//! keystrokes to the child over raw fds, and stops — without holding a byte —
//! at the adoption site, which then replays the bridged output tail into the
//! terminal-screen model so prompt/readiness detection sees what the pane shows.
//!
//! Handoff contract (`ReexecPtyBridge::finish`):
//! - the bridge never closes the inherited master fd; adoption takes ownership;
//! - every byte read from either side is either written or returned in
//!   [`ReexecPtyBridgeReport::pending_output`] / [`ReexecPtyBridgeReport::pending_to_child`];
//! - an escape sequence split across reads is completed within a short drain
//!   bound, otherwise its buffered prefix is returned as pending output, so a
//!   fresh terminal filter never drops half a control sequence;
//! - `Ctrl+D` is dropped while bridging: no prompt can have been observed by
//!   this image yet, and the same `#stale-ctrl-d-arm` contract applies to a
//!   byte buffered in the inherited stdin across `execve`.

use std::time::{Duration, Instant};

/// Most recent bridged (filtered) child output replayed into the screen model.
pub const REEXEC_BRIDGE_REPLAY_CAP: usize = 64 * 1024;
/// Bound on completing a split escape sequence once the bridge is asked to stop.
pub const REEXEC_BRIDGE_DRAIN_BOUND: Duration = Duration::from_millis(250);
/// Largest single write after a readiness edge, so a stop request never waits
/// behind a large blocking write into a slow pane.
const REEXEC_BRIDGE_WRITE_CHUNK: usize = 1024;

/// What the bridge did and what it still owes the adopter.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReexecPtyBridgeReport {
    /// Filtered child-output bytes written to the pane.
    pub bytes_to_output: u64,
    /// Operator input bytes written to the child.
    pub bytes_to_child: u64,
    /// `Ctrl+D` bytes dropped (`#stale-ctrl-d-arm`).
    pub ctrl_d_dropped: u64,
    /// The master reported EOF/EIO (child gone or slave closed).
    pub child_output_closed: bool,
    /// The operator input fd reported EOF or hangup.
    pub input_closed: bool,
    /// Writing to the pane failed; output forwarding stopped.
    pub output_failed: bool,
    /// Bridged output not yet written to the pane; the adopter writes it first.
    pub pending_output: Vec<u8>,
    /// Operator input not yet written to the child; the adopter writes it first.
    pub pending_to_child: Vec<u8>,
    /// Tail of bridged output (at most [`REEXEC_BRIDGE_REPLAY_CAP`] bytes) for
    /// the adopter's terminal-screen model.
    pub replay: Vec<u8>,
    /// How long the bridge serviced the PTY.
    pub serviced: Duration,
}

#[cfg(unix)]
pub use unix::*;

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::ReexecState;
    use crate::pty::PtyFilter;
    use crate::shared_writer::StopSignal;
    use std::os::unix::io::RawFd;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Raw fds the bridge services. It borrows all three; none is closed.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ReexecPtyBridgeFds {
        /// Inherited PTY master of the preserved child.
        pub master: RawFd,
        /// Operator input (the pane's stdin).
        pub input: RawFd,
        /// Pane output (stdout).
        pub output: RawFd,
    }

    /// A running bridge. Dropping it stops and joins the pump; call
    /// [`ReexecPtyBridge::finish`] to also receive the handoff report.
    pub struct ReexecPtyBridge {
        stop: Option<StopSignal>,
        handle: Option<std::thread::JoinHandle<ReexecPtyBridgeReport>>,
        master_fd: RawFd,
        started: Instant,
        child_closed: Arc<AtomicBool>,
    }

    impl ReexecPtyBridge {
        /// Start pumping `fds`. `harness_binary` selects the output filter
        /// policy (`opencode` preserves Kitty keyboard mode); `None` uses the
        /// default policy.
        pub fn start(
            fds: ReexecPtyBridgeFds,
            harness_binary: Option<&str>,
        ) -> anyhow::Result<Self> {
            Self::start_with_drain_bound(fds, harness_binary, REEXEC_BRIDGE_DRAIN_BOUND)
        }

        pub(crate) fn start_with_drain_bound(
            fds: ReexecPtyBridgeFds,
            harness_binary: Option<&str>,
            drain_bound: Duration,
        ) -> anyhow::Result<Self> {
            if !fd_is_open(fds.master) {
                anyhow::bail!("reexec bridge: master fd {} is not open", fds.master);
            }
            let stop = StopSignal::new()?;
            let stop_fd = stop.read_fd();
            let filter = PtyFilter::for_harness_binary(harness_binary.unwrap_or_default());
            let child_closed = Arc::new(AtomicBool::new(false));
            let pump = Pump {
                fds,
                filter,
                filtered: Vec::with_capacity(8192),
                report: ReexecPtyBridgeReport::default(),
                master_open: true,
                input_open: true,
                output_ok: true,
                child_closed: child_closed.clone(),
                drain_bound,
            };
            let handle = std::thread::Builder::new()
                .name("reexec-pty-bridge".into())
                .spawn(move || bridge_loop(pump, stop_fd))?;
            Ok(Self {
                stop: Some(stop),
                handle: Some(handle),
                master_fd: fds.master,
                started: Instant::now(),
                child_closed,
            })
        }

        /// Whether the pump has observed the master close (child gone).
        pub fn child_output_closed(&self) -> bool {
            self.child_closed.load(Ordering::Acquire)
        }

        /// The master fd this bridge services.
        pub fn master_fd(&self) -> RawFd {
            self.master_fd
        }

        /// Stop the pump and return everything the adopter must apply before its
        /// own reader/writer threads start. After this returns, no bridge code
        /// touches any of the fds again.
        pub fn finish(mut self) -> ReexecPtyBridgeReport {
            self.stop_and_join()
        }

        fn stop_and_join(&mut self) -> ReexecPtyBridgeReport {
            if let Some(stop) = self.stop.as_ref() {
                stop.signal();
            }
            let mut report = self
                .handle
                .take()
                .and_then(|handle| handle.join().ok())
                .unwrap_or_default();
            // The stop pipe outlives the thread that polls it.
            self.stop.take();
            report.serviced = self.started.elapsed();
            report
        }
    }

    impl Drop for ReexecPtyBridge {
        fn drop(&mut self) {
            if self.handle.is_some() {
                let _ = self.stop_and_join();
            }
        }
    }

    static EARLY_BRIDGE: parking_lot::Mutex<Option<ReexecPtyBridge>> =
        parking_lot::const_mutex(None);

    /// Start the bridge for a hot-reexec handoff named by the environment, as
    /// the very first thing the replacement image does — before CLI-entry
    /// freshness probes, config loading, or start admission, any of which may
    /// wait on `state.db`. A no-op when this process is not a preserved-child
    /// reentry, the child is gone, or a bridge already runs. The handoff
    /// environment is left in place for start admission to consume.
    ///
    /// `subcommand` is the raw first CLI argument; see
    /// [`early_reexec_bridge_admitted`] for why it matters.
    pub fn start_early_reexec_pty_bridge_from_env(subcommand: Option<&str>) -> bool {
        let Some(state) = ReexecState::from_env() else {
            return false;
        };
        if !early_reexec_bridge_admitted(
            std::env::var(crate::REEXEC_SUPERVISOR_PID_ENV)
                .ok()
                .as_deref(),
            std::process::id(),
            subcommand,
        ) || !state.child_survived()
        {
            return false;
        }
        let harness = std::env::var(crate::REEXEC_HARNESS_ENV).ok();
        install_early_reexec_pty_bridge(
            ReexecPtyBridgeFds {
                master: state.master_fd,
                input: libc::STDIN_FILENO,
                output: libc::STDOUT_FILENO,
            },
            harness.as_deref(),
        )
    }

    /// Whether this process is the replacement image itself rather than a
    /// subprocess that inherited the handoff environment (and the
    /// non-`CLOEXEC` master fd) before start admission scrubbed them. A
    /// subprocess that bridged the master would steal the pane's keystrokes
    /// and the child's output from the real supervisor. `execve` keeps the
    /// PID, so the outgoing image's PID proves identity; an older outgoing
    /// image that does not export it is admitted only for the `start`
    /// supervisor command it replays.
    pub fn early_reexec_bridge_admitted(
        supervisor_pid_env: Option<&str>,
        own_pid: u32,
        subcommand: Option<&str>,
    ) -> bool {
        match supervisor_pid_env {
            Some(pid) => pid.trim().parse::<u32>().ok() == Some(own_pid),
            None => subcommand == Some("start"),
        }
    }

    /// Start a bridge into the process-wide early slot (idempotent).
    pub fn install_early_reexec_pty_bridge(
        fds: ReexecPtyBridgeFds,
        harness_binary: Option<&str>,
    ) -> bool {
        let mut slot = EARLY_BRIDGE.lock();
        if slot.is_some() {
            return true;
        }
        match ReexecPtyBridge::start(fds, harness_binary) {
            Ok(bridge) => {
                *slot = Some(bridge);
                true
            }
            Err(_) => false,
        }
    }

    /// Take the early bridge if it services `master_fd`. A bridge for any other
    /// fd is stopped and discarded: it must never compete with the adopter's
    /// own I/O threads.
    pub fn take_early_reexec_pty_bridge(master_fd: Option<RawFd>) -> Option<ReexecPtyBridge> {
        let bridge = EARLY_BRIDGE.lock().take()?;
        if Some(bridge.master_fd()) == master_fd {
            Some(bridge)
        } else {
            drop(bridge);
            None
        }
    }

    fn fd_is_open(fd: RawFd) -> bool {
        fd >= 0 && unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0
    }

    fn last_errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    fn retryable(errno: i32) -> bool {
        errno == libc::EINTR || errno == libc::EAGAIN || errno == libc::EWOULDBLOCK
    }

    /// Poll `fds`, retrying `EINTR`. Returns false on a hard poll error.
    fn poll_fds(fds: &mut [libc::pollfd], timeout_ms: i32) -> bool {
        loop {
            let ret =
                unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
            if ret >= 0 {
                return true;
            }
            if last_errno() != libc::EINTR {
                return false;
            }
        }
    }

    fn stop_requested(stop_fd: RawFd) -> bool {
        let mut pfd = [libc::pollfd {
            fd: stop_fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        poll_fds(&mut pfd, 0) && pfd[0].revents & libc::POLLIN != 0
    }

    enum WriteOutcome {
        Done,
        /// Stop arrived first; the unwritten suffix starts at this offset.
        Stopped(usize),
        Failed,
    }

    /// Write `data` to `fd` in bounded chunks after each writability edge.
    /// `stop_fd` interrupts a write that cannot make progress; `None` means a
    /// stop has already been requested and the drain must finish regardless.
    fn write_all_or_stop(fd: RawFd, data: &[u8], stop_fd: Option<RawFd>) -> WriteOutcome {
        let mut offset = 0;
        while offset < data.len() {
            let mut pfds = [
                libc::pollfd {
                    fd,
                    events: libc::POLLOUT,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop_fd.unwrap_or(-1),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let count = if stop_fd.is_some() { 2 } else { 1 };
            let timeout = if stop_fd.is_some() {
                -1
            } else {
                REEXEC_BRIDGE_DRAIN_BOUND.as_millis() as i32
            };
            if !poll_fds(&mut pfds[..count], timeout) {
                return WriteOutcome::Failed;
            }
            if pfds[0].revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                return WriteOutcome::Failed;
            }
            if pfds[0].revents & libc::POLLOUT == 0 {
                if stop_fd.is_none() || pfds[1].revents & libc::POLLIN != 0 {
                    return WriteOutcome::Stopped(offset);
                }
                if pfds[0].revents & libc::POLLHUP != 0 {
                    return WriteOutcome::Failed;
                }
                continue;
            }
            let end = (offset + REEXEC_BRIDGE_WRITE_CHUNK).min(data.len());
            let chunk = &data[offset..end];
            let n = unsafe { libc::write(fd, chunk.as_ptr() as *const libc::c_void, chunk.len()) };
            if n > 0 {
                offset += n as usize;
            } else if n < 0 && retryable(last_errno()) {
                continue;
            } else {
                return WriteOutcome::Failed;
            }
        }
        WriteOutcome::Done
    }

    struct Pump {
        fds: ReexecPtyBridgeFds,
        filter: PtyFilter,
        filtered: Vec<u8>,
        report: ReexecPtyBridgeReport,
        master_open: bool,
        input_open: bool,
        output_ok: bool,
        child_closed: Arc<AtomicBool>,
        drain_bound: Duration,
    }

    impl Pump {
        fn mark_child_closed(&mut self) {
            self.master_open = false;
            self.report.child_output_closed = true;
            self.child_closed.store(true, Ordering::Release);
        }

        fn record_replay(&mut self, bytes: &[u8]) {
            let replay = &mut self.report.replay;
            replay.extend_from_slice(bytes);
            if replay.len() > REEXEC_BRIDGE_REPLAY_CAP {
                let excess = replay.len() - REEXEC_BRIDGE_REPLAY_CAP;
                replay.drain(..excess);
            }
        }

        /// Forward filtered output. Returns false when a stop interrupted it.
        fn emit_output(&mut self, bytes: &[u8], stop_fd: Option<RawFd>) -> bool {
            if bytes.is_empty() {
                return true;
            }
            self.record_replay(bytes);
            if !self.output_ok {
                self.report.pending_output.extend_from_slice(bytes);
                return true;
            }
            match write_all_or_stop(self.fds.output, bytes, stop_fd) {
                WriteOutcome::Done => {
                    self.report.bytes_to_output += bytes.len() as u64;
                    true
                }
                WriteOutcome::Stopped(offset) => {
                    self.report.bytes_to_output += offset as u64;
                    self.report
                        .pending_output
                        .extend_from_slice(&bytes[offset..]);
                    false
                }
                WriteOutcome::Failed => {
                    self.output_ok = false;
                    self.report.output_failed = true;
                    self.report.pending_output.extend_from_slice(bytes);
                    true
                }
            }
        }

        /// Read once from the master and forward. Returns false when a stop
        /// interrupted the forward.
        fn pump_master(&mut self, buf: &mut [u8], stop_fd: Option<RawFd>) -> bool {
            let n = unsafe {
                libc::read(
                    self.fds.master,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n == 0 || (n < 0 && !retryable(last_errno())) {
                self.mark_child_closed();
                return true;
            }
            if n < 0 {
                return true;
            }
            let mut filtered = std::mem::take(&mut self.filtered);
            filtered.clear();
            self.filter.filter(&buf[..n as usize], &mut filtered);
            let completed = self.emit_output(&filtered, stop_fd);
            self.filtered = filtered;
            completed
        }

        /// Read once from operator input and forward to the child. Returns
        /// false when a stop interrupted the forward.
        fn pump_input(&mut self, buf: &mut [u8], stop_fd: RawFd) -> bool {
            let n = unsafe {
                libc::read(
                    self.fds.input,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n == 0 || (n < 0 && !retryable(last_errno())) {
                self.input_open = false;
                self.report.input_closed = true;
                return true;
            }
            if n < 0 {
                return true;
            }
            let data = &buf[..n as usize];
            let dropped = data.iter().filter(|byte| **byte == 0x04).count();
            self.report.ctrl_d_dropped += dropped as u64;
            let data: Vec<u8> = data.iter().copied().filter(|byte| *byte != 0x04).collect();
            if data.is_empty() {
                return true;
            }
            match write_all_or_stop(self.fds.master, &data, Some(stop_fd)) {
                WriteOutcome::Done => {
                    self.report.bytes_to_child += data.len() as u64;
                    true
                }
                WriteOutcome::Stopped(offset) => {
                    self.report.bytes_to_child += offset as u64;
                    self.report
                        .pending_to_child
                        .extend_from_slice(&data[offset..]);
                    false
                }
                WriteOutcome::Failed => {
                    self.mark_child_closed();
                    self.report.pending_to_child.extend_from_slice(&data);
                    true
                }
            }
        }

        /// After a stop: finish a split escape sequence within the drain bound,
        /// then hand any remaining prefix to the adopter as pending output.
        fn drain_partial_sequence(&mut self, buf: &mut [u8]) {
            let deadline = Instant::now() + self.drain_bound;
            while self.master_open && !self.filter.at_sequence_boundary() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let mut pfd = [libc::pollfd {
                    fd: self.fds.master,
                    events: libc::POLLIN,
                    revents: 0,
                }];
                if !poll_fds(&mut pfd, remaining.as_millis().max(1) as i32) {
                    break;
                }
                if pfd[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) == 0 {
                    break;
                }
                self.pump_master(buf, None);
            }
            let carryover = self.filter.take_carryover();
            if !carryover.is_empty() {
                self.record_replay(&carryover);
                self.report.pending_output.extend_from_slice(&carryover);
            }
        }
    }

    fn bridge_loop(mut pump: Pump, stop_fd: RawFd) -> ReexecPtyBridgeReport {
        let fds = pump.fds;
        let mut buf = vec![0u8; 8192];
        loop {
            let mut pfds = Vec::with_capacity(3);
            pfds.push(libc::pollfd {
                fd: stop_fd,
                events: libc::POLLIN,
                revents: 0,
            });
            let master_slot = (pump.master_open && pump.output_ok).then(|| {
                pfds.push(libc::pollfd {
                    fd: fds.master,
                    events: libc::POLLIN,
                    revents: 0,
                });
                pfds.len() - 1
            });
            let input_slot = (pump.input_open && pump.master_open).then(|| {
                pfds.push(libc::pollfd {
                    fd: fds.input,
                    events: libc::POLLIN,
                    revents: 0,
                });
                pfds.len() - 1
            });
            if !poll_fds(&mut pfds, -1) {
                break;
            }
            if pfds[0].revents & libc::POLLIN != 0 {
                break;
            }
            if let Some(slot) = master_slot {
                let revents = pfds[slot].revents;
                if revents & libc::POLLNVAL != 0 {
                    pump.mark_child_closed();
                } else if revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0
                    && !pump.pump_master(&mut buf, Some(stop_fd))
                {
                    break;
                }
            }
            // Input for a child whose output just closed stays unread: the
            // adopter's restart path owns stdin from here.
            if let Some(slot) = input_slot.filter(|_| pump.master_open) {
                let revents = pfds[slot].revents;
                if revents & libc::POLLIN != 0 {
                    if !pump.pump_input(&mut buf, stop_fd) {
                        break;
                    }
                } else if revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                    pump.input_open = false;
                    pump.report.input_closed = true;
                }
            }
            if stop_requested(stop_fd) {
                break;
            }
        }
        pump.drain_partial_sequence(&mut buf);
        pump.report
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::io::RawFd;

    /// A connected socketpair stands in for the PTY master/slave pair: both
    /// ends are bidirectional, pollable fds, so the bridge runs its real
    /// syscall path against a deterministic "child" the test drives.
    fn socketpair() -> (RawFd, RawFd) {
        let mut fds = [0; 2];
        assert_eq!(
            unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) },
            0
        );
        (fds[0], fds[1])
    }

    fn pipe() -> (RawFd, RawFd) {
        let mut fds = [0; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        (fds[0], fds[1])
    }

    fn write_fd(fd: RawFd, data: &[u8]) {
        let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
        assert_eq!(n as usize, data.len());
    }

    /// Read until `want` bytes arrive or a 2 s deadline passes.
    fn read_until(fd: RawFd, want: usize) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while out.len() < want && Instant::now() < deadline {
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut pfd, 1, 50) } <= 0 {
                continue;
            }
            let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
        out
    }

    fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn unread_bytes(fd: RawFd) -> i32 {
        let mut n: libc::c_int = 0;
        assert_eq!(unsafe { libc::ioctl(fd, libc::FIONREAD, &mut n) }, 0);
        n
    }

    fn close(fd: RawFd) {
        unsafe { libc::close(fd) };
    }

    fn fd_open(fd: RawFd) -> bool {
        (unsafe { libc::fcntl(fd, libc::F_GETFD) }) >= 0
    }

    struct World {
        master: RawFd,
        child: RawFd,
        input_r: RawFd,
        input_w: RawFd,
        output_r: RawFd,
        output_w: RawFd,
    }

    impl World {
        fn new() -> Self {
            let (master, child) = socketpair();
            let (input_r, input_w) = pipe();
            let (output_r, output_w) = pipe();
            Self {
                master,
                child,
                input_r,
                input_w,
                output_r,
                output_w,
            }
        }

        fn fds(&self) -> ReexecPtyBridgeFds {
            ReexecPtyBridgeFds {
                master: self.master,
                input: self.input_r,
                output: self.output_w,
            }
        }
    }

    impl Drop for World {
        fn drop(&mut self) {
            for fd in [
                self.master,
                self.child,
                self.input_r,
                self.input_w,
                self.output_r,
                self.output_w,
            ] {
                close(fd);
            }
        }
    }

    #[test]
    fn bridge_services_the_pty_while_startup_is_blocked() {
        // GH #270: the replacement image's startup waited on `state.db` for
        // 31-90 s while nothing serviced the inherited master. The bridge runs
        // independently of that thread: a startup that cannot make progress
        // (stood in for by this test thread not touching the bridge) must not
        // stop keystrokes reaching the child or child output reaching the pane.
        const STREAMING: &[u8] = "\u{273b} Stewing\u{2026} (3m 6s) streaming output".as_bytes();
        let world = World::new();
        let bridge = ReexecPtyBridge::start(world.fds(), Some("claude")).unwrap();

        write_fd(world.child, STREAMING);
        assert_eq!(read_until(world.output_r, STREAMING.len()), STREAMING);
        write_fd(world.input_w, b"hello");
        assert_eq!(read_until(world.child, 5), b"hello");

        let report = bridge.finish();
        assert_eq!(report.bytes_to_child, 5);
        assert_eq!(report.bytes_to_output, STREAMING.len() as u64);
        assert!(report.pending_output.is_empty());
        assert!(report.pending_to_child.is_empty());
        assert_eq!(report.replay, STREAMING);
        assert!(
            fd_open(world.master),
            "adoption takes ownership of the master; the bridge must never close it"
        );
    }

    #[test]
    fn finish_hands_off_without_consuming_later_bytes() {
        let world = World::new();
        let bridge = ReexecPtyBridge::start(world.fds(), None).unwrap();
        write_fd(world.child, b"before");
        assert_eq!(read_until(world.output_r, 6), b"before");
        let _ = bridge.finish();

        // After the handoff every byte belongs to the adopter's own threads.
        write_fd(world.child, b"after");
        write_fd(world.input_w, b"keys");
        assert_eq!(read_until(world.master, 5), b"after");
        assert_eq!(read_until(world.input_r, 4), b"keys");
    }

    #[test]
    fn bridge_drops_stale_ctrl_d_and_filters_terminal_queries() {
        let world = World::new();
        let bridge = ReexecPtyBridge::start(world.fds(), Some("claude")).unwrap();
        write_fd(world.input_w, b"a\x04b");
        assert_eq!(read_until(world.child, 2), b"ab");
        // A DA query (`ESC[c`) is filtered exactly as the reader thread does.
        write_fd(world.child, b"x\x1b[cy");
        assert_eq!(read_until(world.output_r, 2), b"xy");
        let report = bridge.finish();
        assert_eq!(report.ctrl_d_dropped, 1);
        assert_eq!(report.replay, b"xy");
    }

    #[test]
    fn split_escape_sequence_is_completed_or_handed_off() {
        // Completed within the drain bound: the whole sequence passes through.
        // The bound is widened so a loaded host cannot turn this into the
        // second case; the drain still returns as soon as the sequence ends.
        let world = World::new();
        let bridge =
            ReexecPtyBridge::start_with_drain_bound(world.fds(), None, Duration::from_secs(10))
                .unwrap();
        write_fd(world.child, b"ok\x1b[3");
        assert_eq!(read_until(world.output_r, 2), b"ok");
        let finisher = std::thread::spawn(move || bridge.finish());
        std::thread::sleep(Duration::from_millis(20));
        write_fd(world.child, b"1mred");
        let report = finisher.join().unwrap();
        assert!(report.pending_output.is_empty(), "{report:?}");
        assert_eq!(read_until(world.output_r, 8), b"\x1b[31mred");

        // Never completed: the prefix goes to the adopter instead of vanishing.
        let world = World::new();
        let bridge = ReexecPtyBridge::start(world.fds(), None).unwrap();
        write_fd(world.child, b"ok\x1b[3");
        assert_eq!(read_until(world.output_r, 2), b"ok");
        let report = bridge.finish();
        assert_eq!(report.pending_output, b"\x1b[3");
    }

    #[test]
    fn child_exit_leaves_later_input_for_the_adopter() {
        let world = World::new();
        let bridge = ReexecPtyBridge::start(world.fds(), None).unwrap();
        write_fd(world.child, b"bye");
        assert_eq!(read_until(world.output_r, 3), b"bye");
        unsafe { libc::shutdown(world.child, libc::SHUT_RDWR) };
        wait_for("bridge to observe the child hangup", || {
            bridge.child_output_closed()
        });
        write_fd(world.input_w, b"late");
        let report = bridge.finish();
        assert!(report.child_output_closed);
        assert!(report.pending_to_child.is_empty());
        // Input for a lost child is not consumed; the adopter's restart path
        // owns stdin from here.
        assert_eq!(read_until(world.input_r, 4), b"late");
    }

    #[test]
    fn a_stalled_pane_cannot_wedge_the_handoff() {
        // A pane that stops draining output must not hold the adopter forever:
        // stop interrupts the bounded write and the remainder is handed over.
        let world = World::new();
        let filler = vec![b'z'; 1 << 20];
        // Fill the output pipe so the bridge's write blocks on readiness.
        unsafe {
            let flags = libc::fcntl(world.output_w, libc::F_GETFL);
            libc::fcntl(world.output_w, libc::F_SETFL, flags | libc::O_NONBLOCK);
            while libc::write(
                world.output_w,
                filler.as_ptr() as *const libc::c_void,
                filler.len(),
            ) > 0
            {}
            libc::fcntl(world.output_w, libc::F_SETFL, flags);
        }
        let bridge = ReexecPtyBridge::start(world.fds(), None).unwrap();
        write_fd(world.child, b"blocked");
        wait_for("bridge to consume the child output", || {
            unread_bytes(world.master) == 0
        });
        let started = Instant::now();
        let report = bridge.finish();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(report.pending_output, b"blocked");
        assert_eq!(report.replay, b"blocked");
    }

    #[test]
    fn early_slot_is_taken_only_for_the_adopted_master() {
        let world = World::new();
        assert!(install_early_reexec_pty_bridge(world.fds(), None));
        assert!(
            take_early_reexec_pty_bridge(Some(world.master + 1000)).is_none(),
            "a bridge for another fd is stopped, never handed to the adopter"
        );
        assert!(take_early_reexec_pty_bridge(Some(world.master)).is_none());

        assert!(install_early_reexec_pty_bridge(world.fds(), None));
        let bridge = take_early_reexec_pty_bridge(Some(world.master)).unwrap();
        assert_eq!(bridge.master_fd(), world.master);
        drop(bridge);
    }

    #[test]
    fn only_the_replacement_image_itself_bridges_early() {
        assert!(early_reexec_bridge_admitted(
            Some("4242"),
            4242,
            Some("start")
        ));
        assert!(
            early_reexec_bridge_admitted(Some("4242"), 4242, Some("init-session")),
            "the PID proof does not depend on how the supervisor was launched"
        );
        assert!(
            !early_reexec_bridge_admitted(Some("4242"), 5151, Some("start")),
            "a subprocess inherited the env and the master fd: never bridge"
        );
        assert!(!early_reexec_bridge_admitted(
            Some("garbage"),
            4242,
            Some("start")
        ));
        assert!(early_reexec_bridge_admitted(None, 4242, Some("start")));
        assert!(
            !early_reexec_bridge_admitted(None, 4242, Some("commit")),
            "an older outgoing image is trusted only for the supervisor command"
        );
        assert!(!early_reexec_bridge_admitted(None, 4242, None));
    }

    #[test]
    fn start_refuses_a_closed_master() {
        assert!(
            ReexecPtyBridge::start(
                ReexecPtyBridgeFds {
                    master: -1,
                    input: libc::STDIN_FILENO,
                    output: libc::STDOUT_FILENO,
                },
                None,
            )
            .is_err()
        );
    }
}

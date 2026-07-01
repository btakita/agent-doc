//! # Module: supervisor::pty
//!
//! Pty allocation and child spawning for the supervisor.
//!
//! ## Spec
//! See `src/agent-doc/specs/supervisor.md` § Architecture / Pty lifecycle.
//!
//! This module owns three responsibilities, in order:
//!
//! 1. **Allocate a pty pair** via `portable-pty` (Unix pty on Linux/macOS,
//!    ConPTY on Windows — the supervisor is a supported target on both).
//! 2. **Spawn the child process** (claude) with the slave as its controlling
//!    tty, setting CWD and env deterministically from caller-supplied config.
//!    The slave handle is dropped after spawn, per portable-pty convention, so
//!    the child owns the only reference and EOFs cleanly on exit.
//! 3. **Optionally start stdin→master and master→stdout forwarding threads**
//!    (`forward_stdio`). Callers that want full bidirectional I/O (i.e., real
//!    supervisor runs from `start.rs`) call this after `spawn`. Integration
//!    tests that drive the child programmatically skip it and wait on `wait()`
//!    directly.
//!
//! ## Scope boundary
//!
//! Resize handling (SIGWINCH → `master.resize`) lives in the sibling
//! `resize.rs` module. This module exposes a thin `resize()` wrapper so
//! `resize.rs` never has to reach into `portable-pty` types directly — it
//! stays coupled only to `PtySession`.
//!
//! Crash classification, restart policy, and the IPC accept loop also live in
//! sibling modules (`state.rs`, `ipc.rs`). `pty.rs` is deliberately narrow:
//! spawn, forward, wait, resize, kill.
//!
//! ## Invariants
//!
//! - The caller-supplied `cwd` is canonicalized in `cwd.rs` before reaching
//!   this module. We trust it and pass it to `CommandBuilder::cwd` unchanged.
//! - The caller-supplied `env` is the complete env for the child. Parent env
//!   is **not** inherited — this enforces the "deterministic env" invariant
//!   from the spec. Callers that want PATH, HOME, etc. populate them
//!   explicitly.
//! - The slave side of the pty is dropped immediately after spawn so the
//!   child is the sole owner; without this the master reader never sees EOF
//!   when the child exits.
//! - I/O forwarding threads exit when their underlying pipe closes. They are
//!   not torn down explicitly — the pty master drop on `PtySession::drop`
//!   signals EOF to both sides.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::thread::{self, JoinHandle};

use agent_doc_supervisor::terminal_filter::{
    TerminalFilter, TerminalFilterAction, TerminalFilterConfig, TerminalFilterTrace,
    TerminalFilterTraceKind,
};
use anyhow::{Context, Result};
use portable_pty::{
    Child, ChildKiller, CommandBuilder, ExitStatus, MasterPty, NativePtySystem, PtySize, PtySystem,
};

/// A thread-safe handle for resizing a pty session from another thread.
///
/// On Unix, stores the raw master fd and calls `TIOCSWINSZ` directly.
/// This avoids the need for `MasterPty` to be `Sync` (which portable-pty
/// doesn't provide).
#[cfg(unix)]
pub struct ResizeHandle {
    fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl ResizeHandle {
    /// Resize the pty to the given dimensions.
    pub fn resize(&self, size: PtySize) -> Result<()> {
        let ws = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.pixel_width,
            ws_ypixel: size.pixel_height,
        };
        let ret = unsafe { libc::ioctl(self.fd, libc::TIOCSWINSZ, &ws) };
        if ret != 0 {
            anyhow::bail!(
                "TIOCSWINSZ ioctl failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }
}

#[cfg(unix)]
impl Drop for ResizeHandle {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// Stub resize handle for non-Unix platforms.
#[cfg(not(unix))]
pub struct ResizeHandle;

#[cfg(not(unix))]
impl ResizeHandle {
    pub fn resize(&self, _size: PtySize) -> Result<()> {
        eprintln!("[supervisor::pty] resize not supported on this platform");
        Ok(())
    }
}

/// Configuration for spawning a child process under a pty.
///
/// Built by the caller (typically `start.rs` after `cwd::resolve`) and
/// consumed by [`PtySession::spawn`].
pub struct PtySpawnConfig {
    /// Program to execute (e.g. `"claude"`). Looked up via `$PATH` in the
    /// parent environment only if the caller includes `PATH` in `env`.
    pub program: String,
    /// Arguments passed to the program, in order.
    pub args: Vec<String>,
    /// Working directory for the child. Must already be canonicalized.
    pub cwd: PathBuf,
    /// Complete environment for the child. Parent env is **not** inherited.
    pub env: HashMap<String, String>,
    /// Initial pty size. Resize events land through [`PtySession::resize`].
    pub size: PtySize,
}

impl PtySpawnConfig {
    /// Construct a config with a default 24×80 pty size. Callers that know
    /// the real terminal size (from tmux, SIGWINCH, or the IPC client) should
    /// override `size` before calling `spawn`.
    #[allow(dead_code)] // convenience constructor — used by tests
    pub fn new(program: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd,
            env: HashMap::new(),
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        }
    }
}

/// A running child process under a pty owned by the supervisor.
///
/// Holds the master side of the pty, the child handle, and any I/O
/// forwarding threads started via [`PtySession::forward_stdio`]. Drop order:
/// threads finish when the master/child closes, then the master and child
/// handles drop.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    /// The child handle. `Some` until [`take_child`](Self::take_child) hands it
    /// to the in-process supervisor adapter (`#pcpc5e1` authority rung), which
    /// then owns non-blocking exit reaping (`try_wait`) and `kill`. In the
    /// default out-of-process host path this stays `Some` and [`wait`](Self::wait)
    /// reaps it inline.
    child: Option<Box<dyn Child + Send + Sync>>,
    #[allow(dead_code)] // used by forward_stdio (test path)
    io_threads: Vec<JoinHandle<()>>,
}

impl PtySession {
    /// Allocate a pty pair and spawn the child process.
    ///
    /// On success, the child is running and the caller can either:
    /// - Call [`forward_stdio`](Self::forward_stdio) to wire stdin/stdout to
    ///   the pty (real supervisor runs).
    /// - Call [`wait`](Self::wait) directly (integration tests driving
    ///   programmatic children).
    pub fn spawn(cfg: PtySpawnConfig) -> Result<Self> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(cfg.size)
            .with_context(|| "openpty: failed to allocate pty pair")?;

        let mut cmd = CommandBuilder::new(&cfg.program);
        for arg in &cfg.args {
            cmd.arg(arg);
        }
        cmd.cwd(&cfg.cwd);
        cmd.env_clear();
        for (k, v) in &cfg.env {
            cmd.env(k, v);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("spawn_command failed for program: {}", cfg.program))?;

        // Drop the slave immediately: the child now owns the only reference.
        // Without this, master reads block forever on child exit because the
        // slave fd is still open in the supervisor process.
        drop(pair.slave);

        Ok(Self {
            master: pair.master,
            child: Some(child),
            io_threads: Vec::new(),
        })
    }

    /// Start the two I/O forwarding threads (stdin→master, master→stdout).
    ///
    /// Call at most once per session. Subsequent calls return an error
    /// because the writer can only be taken once from the master.
    #[allow(dead_code)] // used by tests; start.rs does I/O manually for shared inject writer
    pub fn forward_stdio(&mut self) -> Result<()> {
        if !self.io_threads.is_empty() {
            anyhow::bail!("forward_stdio called twice on the same PtySession");
        }

        let mut reader = self
            .master
            .try_clone_reader()
            .context("try_clone_reader: failed to clone pty reader for master→stdout thread")?;
        let mut writer = self
            .master
            .take_writer()
            .context("take_writer: failed to take pty writer for stdin→master thread")?;

        let out_thread = thread::Builder::new()
            .name("pty->stdout".into())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                let stdout = std::io::stdout();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break, // child closed slave
                        Ok(n) => {
                            let mut lock = stdout.lock();
                            if let Err(e) = lock.write_all(&buf[..n]) {
                                eprintln!("[supervisor::pty] stdout write error: {e}");
                                break;
                            }
                            if let Err(e) = lock.flush() {
                                eprintln!("[supervisor::pty] stdout flush error: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("[supervisor::pty] master read error: {e}");
                            break;
                        }
                    }
                }
            })
            .context("spawn pty->stdout thread")?;

        let in_thread = thread::Builder::new()
            .name("stdin->pty".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                let stdin = std::io::stdin();
                loop {
                    let mut lock = stdin.lock();
                    match lock.read(&mut buf) {
                        Ok(0) => break, // parent stdin closed
                        Ok(n) => {
                            if agent_doc_tmux_commands::input_diag::verbose_enabled() {
                                agent_doc_tmux_io::input_diag::log_byte_events(
                                    agent_doc_tmux_io::input_diag::InputDiagSink::new(
                                        None,
                                        crate::ops_log::log_op,
                                    ),
                                    "supervisor.forward_stdio",
                                    "child_pty",
                                    "raw_forward",
                                    &buf[..n],
                                    None,
                                );
                            }
                            if let Err(e) = writer.write_all(&buf[..n]) {
                                eprintln!("[supervisor::pty] pty write error: {e}");
                                break;
                            }
                            if let Err(e) = writer.flush() {
                                eprintln!("[supervisor::pty] pty flush error: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("[supervisor::pty] stdin read error: {e}");
                            break;
                        }
                    }
                }
            })
            .context("spawn stdin->pty thread")?;

        self.io_threads.push(out_thread);
        self.io_threads.push(in_thread);
        Ok(())
    }

    /// Resize the pty. Called by `resize.rs` on SIGWINCH (Unix) or
    /// `ReadConsoleInputW` window events (Windows).
    #[allow(dead_code)] // used by tests; start.rs uses ResizeHandle for cross-thread resize
    pub fn resize(&self, size: PtySize) -> Result<()> {
        self.master
            .resize(size)
            .with_context(|| format!("pty resize to {}x{} failed", size.rows, size.cols))
    }

    /// Block until the child exits and return its exit status.
    ///
    /// Mutable because `portable-pty`'s `Child::wait` takes `&mut self`.
    pub fn wait(&mut self) -> Result<ExitStatus> {
        self.child
            .as_mut()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "PtySession::wait called after take_child — the in-process supervisor adapter now owns child reaping"
                )
            })?
            .wait()
            .context("child.wait failed")
    }

    /// Hand the child handle to the in-process supervisor adapter
    /// ([`super::in_process::PtySupervisedChild`], `#pcpc5e1` authority rung).
    /// The master (and its reader/writer/resize handles) stays with this
    /// `PtySession`, so the caller keeps PTY-output/prompt plumbing while the
    /// adapter drives non-blocking exit reaping (`try_wait`) and `kill`. After
    /// this call [`wait`](Self::wait) and [`kill`](Self::kill) fail (the child is
    /// gone); dropping the session then only closes the master, signalling reader
    /// EOF. Returns an error if the child was already taken.
    pub fn take_child(&mut self) -> Result<Box<dyn Child + Send + Sync>> {
        self.child
            .take()
            .ok_or_else(|| anyhow::anyhow!("PtySession::take_child called twice"))
    }

    /// Attempt to kill the child. Used by the IPC `stop` handler and on
    /// supervisor shutdown when the child refuses to exit via SIGHUP.
    #[allow(dead_code)] // API surface — used by tests; IPC stop uses libc::kill via PID
    pub fn kill(&mut self) -> Result<()> {
        match self.child.as_mut() {
            Some(child) => child.kill().context("child.kill failed"),
            // Child already handed to the in-process adapter, which owns kill.
            None => Ok(()),
        }
    }

    /// Create a thread-safe resize handle that can be sent to other threads
    /// (e.g., [`agent_doc_supervisor_process::resize::ResizeWatcher`]).
    ///
    /// On Unix, extracts the raw master fd via `MasterPty::as_raw_fd` and
    /// returns a handle that calls `TIOCSWINSZ` directly. This avoids
    /// needing `MasterPty: Sync`.
    #[cfg(unix)]
    pub fn resize_handle(&self) -> Result<ResizeHandle> {
        let fd = self
            .master
            .as_raw_fd()
            .ok_or_else(|| anyhow::anyhow!("master pty does not expose a raw fd for resize"))?;
        // dup the fd so the handle remains valid even if the session is dropped
        let duped = unsafe { libc::dup(fd) };
        if duped < 0 {
            anyhow::bail!("dup(master_fd) failed: {}", std::io::Error::last_os_error());
        }
        Ok(ResizeHandle { fd: duped })
    }

    #[cfg(not(unix))]
    pub fn resize_handle(&self) -> Result<ResizeHandle> {
        Ok(ResizeHandle)
    }

    /// Take the pty writer handle for external use (e.g., shared IPC inject
    /// writer). After calling this, [`forward_stdio`](Self::forward_stdio)
    /// will fail because the writer has already been consumed.
    pub fn take_writer(&self) -> Result<Box<dyn Write + Send>> {
        self.master
            .take_writer()
            .context("take_writer: failed to take pty writer")
    }

    /// Duplicate the master fd for direct interruptible writes on Unix.
    #[cfg(unix)]
    pub fn dup_write_fd(&self) -> Result<std::os::unix::io::RawFd> {
        let fd = self
            .master
            .as_raw_fd()
            .ok_or_else(|| anyhow::anyhow!("master pty does not expose a raw fd for writing"))?;
        let duped = unsafe { libc::dup(fd) };
        if duped < 0 {
            anyhow::bail!(
                "dup master fd for write failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(duped)
    }

    /// Clone the pty reader for external use (e.g., master→stdout I/O thread).
    pub fn clone_reader(&self) -> Result<Box<dyn Read + Send>> {
        self.master
            .try_clone_reader()
            .context("try_clone_reader: failed to clone pty reader")
    }

    /// Get the child's process ID, if available.
    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.process_id())
    }

    /// `#ctlrecycle` R3 — reconstruct a session from a pty master fd and child PID
    /// inherited across an in-place `execve` (supervisor binary hot-reload).
    ///
    /// The original `portable_pty` master/child objects do not survive the image
    /// swap, but the OS pty master fd (CLOEXEC cleared before exec) and the child
    /// process (PID preserved — `execve` never reparents children) do. [`AdoptedMaster`]
    /// re-wraps the raw fd and [`RawPidChild`] drives `waitpid`/`kill` over the bare
    /// PID, so the rest of the supervisor (reader/writer/resize/in-process adapter)
    /// operates on the same `dyn MasterPty`/`dyn Child` trait objects unchanged.
    #[cfg(unix)]
    pub fn adopt(master_fd: std::os::unix::io::RawFd, child_pid: u32) -> Result<Self> {
        if master_fd < 0 {
            anyhow::bail!("PtySession::adopt: invalid master fd {master_fd}");
        }
        if child_pid == 0 {
            anyhow::bail!("PtySession::adopt: invalid child pid 0");
        }
        // SAFETY: `master_fd` is an open pty master fd inherited across `execve`; this
        // session takes sole ownership of it (closed when the session drops).
        let master = unsafe { AdoptedMaster::from_raw_fd(master_fd) };
        Ok(Self {
            master: Box::new(master),
            child: Some(Box::new(RawPidChild::new(child_pid))),
            io_threads: Vec::new(),
        })
    }
}

/// `#ctlrecycle` R3 — a pty master reconstructed from a raw fd inherited across an
/// in-place `execve`. The supervisor's reader/writer/resize plumbing only needs the
/// raw fd (it already dups it for the resize handle and direct writes), so after the
/// image swap we re-wrap the surviving OS fd here instead of the lost
/// `portable_pty` master object.
#[cfg(unix)]
#[derive(Debug)]
struct AdoptedMaster {
    fd: std::os::unix::io::OwnedFd,
}

#[cfg(unix)]
impl AdoptedMaster {
    /// SAFETY: `fd` must be an open, owned pty master fd. Takes ownership.
    unsafe fn from_raw_fd(fd: std::os::unix::io::RawFd) -> Self {
        use std::os::unix::io::FromRawFd;
        Self {
            // SAFETY: caller guarantees `fd` is an open, owned fd transferred here.
            fd: unsafe { std::os::unix::io::OwnedFd::from_raw_fd(fd) },
        }
    }

    fn raw(&self) -> std::os::unix::io::RawFd {
        use std::os::unix::io::AsRawFd;
        self.fd.as_raw_fd()
    }

    /// Dup the master fd into an owned `File` for an independent reader/writer.
    /// Dropping the returned `File` closes only the dup, never the master, so a
    /// dropped writer does not EOF the slave out from under the live child.
    fn dup_file(&self) -> Result<std::fs::File> {
        use std::os::unix::io::FromRawFd;
        let duped = unsafe { libc::dup(self.raw()) };
        if duped < 0 {
            anyhow::bail!(
                "dup(adopted master fd) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(unsafe { std::fs::File::from_raw_fd(duped) })
    }
}

#[cfg(unix)]
impl MasterPty for AdoptedMaster {
    fn tty_name(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn resize(&self, size: PtySize) -> Result<()> {
        let ws = libc::winsize {
            ws_row: size.rows,
            ws_col: size.cols,
            ws_xpixel: size.pixel_width,
            ws_ypixel: size.pixel_height,
        };
        let ret = unsafe { libc::ioctl(self.raw(), libc::TIOCSWINSZ, &ws) };
        if ret != 0 {
            anyhow::bail!(
                "TIOCSWINSZ ioctl failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(())
    }

    fn get_size(&self) -> Result<PtySize> {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::ioctl(self.raw(), libc::TIOCGWINSZ, &mut ws) };
        if ret != 0 {
            anyhow::bail!(
                "TIOCGWINSZ ioctl failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok(PtySize {
            rows: ws.ws_row,
            cols: ws.ws_col,
            pixel_width: ws.ws_xpixel,
            pixel_height: ws.ws_ypixel,
        })
    }

    fn try_clone_reader(&self) -> Result<Box<dyn Read + Send>> {
        Ok(Box::new(self.dup_file()?))
    }

    fn take_writer(&self) -> Result<Box<dyn Write + Send>> {
        Ok(Box::new(self.dup_file()?))
    }

    fn process_group_leader(&self) -> Option<libc::pid_t> {
        let pg = unsafe { libc::tcgetpgrp(self.raw()) };
        if pg < 0 { None } else { Some(pg) }
    }

    fn as_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        Some(self.raw())
    }
}

/// `#ctlrecycle` R3 — a child handle backed by a bare PID inherited across an
/// in-place `execve`. `waitpid`/`kill` operate directly on the PID; the supervisor's
/// in-process adapter drives `try_wait`/`kill` exactly as it does for a
/// `portable_pty` child.
#[cfg(unix)]
#[derive(Debug)]
struct RawPidChild {
    pid: u32,
    /// Cached terminal status so repeated `try_wait`/`wait` after reaping are stable
    /// (a second `waitpid` on a reaped PID returns `ECHILD`).
    exited: Option<ExitStatus>,
}

#[cfg(unix)]
impl RawPidChild {
    fn new(pid: u32) -> Self {
        Self { pid, exited: None }
    }

    fn waitpid(&mut self, block: bool) -> std::io::Result<Option<ExitStatus>> {
        if let Some(status) = self.exited.clone() {
            return Ok(Some(status));
        }
        let mut wstatus: libc::c_int = 0;
        let flags = if block { 0 } else { libc::WNOHANG };
        let ret = unsafe { libc::waitpid(self.pid as libc::pid_t, &mut wstatus, flags) };
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if ret == 0 {
            // WNOHANG and the child is still running.
            return Ok(None);
        }
        let status = exit_status_from_wstatus(wstatus);
        self.exited = Some(status.clone());
        Ok(Some(status))
    }
}

/// Map a raw `waitpid` status word to a `portable_pty::ExitStatus`. Signal deaths
/// encode as the shell `128 + signal` convention so a killed child reads as non-zero.
#[cfg(unix)]
fn exit_status_from_wstatus(wstatus: libc::c_int) -> ExitStatus {
    if libc::WIFEXITED(wstatus) {
        ExitStatus::with_exit_code(libc::WEXITSTATUS(wstatus) as u32)
    } else if libc::WIFSIGNALED(wstatus) {
        ExitStatus::with_exit_code(128u32 + libc::WTERMSIG(wstatus) as u32)
    } else {
        ExitStatus::with_exit_code(0)
    }
}

/// Send `SIGHUP` to a bare PID, treating `ESRCH` (already gone) as success — the
/// same convention the supervisor's IPC `stop` path uses for PID-based kills.
#[cfg(unix)]
fn raw_pid_kill(pid: u32) -> std::io::Result<()> {
    let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGHUP) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

#[cfg(unix)]
impl ChildKiller for RawPidChild {
    fn kill(&mut self) -> std::io::Result<()> {
        raw_pid_kill(self.pid)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(RawPidKiller { pid: self.pid })
    }
}

#[cfg(unix)]
impl Child for RawPidChild {
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.waitpid(false)
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        match self.waitpid(true)? {
            Some(status) => Ok(status),
            None => Ok(ExitStatus::with_exit_code(0)),
        }
    }

    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }
}

/// Detachable killer for [`RawPidChild`] so a thread can signal the child while
/// another thread blocks in `wait` (the `ChildKiller::clone_killer` contract).
#[cfg(unix)]
#[derive(Debug)]
struct RawPidKiller {
    pid: u32,
}

#[cfg(unix)]
impl ChildKiller for RawPidKiller {
    fn kill(&mut self) -> std::io::Result<()> {
        raw_pid_kill(self.pid)
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(RawPidKiller { pid: self.pid })
    }
}

/// Effect adapter for the pure terminal filter policy.
pub(crate) struct PtyFilter {
    inner: TerminalFilter,
    trace: Vec<TerminalFilterTrace>,
}

impl PtyFilter {
    pub fn for_harness(harness: &agent_doc_harness::HarnessConfig) -> Self {
        Self {
            inner: TerminalFilter::with_config(TerminalFilterConfig {
                preserve_kitty_keyboard: harness.binary == "opencode",
            }),
            trace: Vec::new(),
        }
    }

    pub fn filter(&mut self, input: &[u8], output: &mut Vec<u8>) {
        self.trace.clear();
        self.inner.filter_with_trace(input, output, &mut self.trace);
        for trace in self.trace.iter().copied() {
            log_terminal_filter_trace(trace);
        }
    }
}

fn log_terminal_filter_trace(trace: TerminalFilterTrace) {
    agent_doc_tmux_io::input_diag::log_key_event_verbose(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, crate::ops_log::log_op),
        "supervisor.pty_filter",
        "stdout",
        match trace.action {
            TerminalFilterAction::Drop => "kitty_keyboard_drop",
            TerminalFilterAction::Preserve => "kitty_keyboard_preserve",
        },
        match trace.kind {
            TerminalFilterTraceKind::KittyKeyboardPush => "kitty_keyboard_push",
            TerminalFilterTraceKind::KittyProgressiveEnhancement => "kitty_progressive_enhancement",
            TerminalFilterTraceKind::KittyKeyboardPop => "kitty_keyboard_pop",
        },
        trace.sequence_len,
        agent_doc_tmux_commands::input_diag::KeyEventMeta {
            harness: None,
            detail: Some(if trace.preserve_kitty_keyboard {
                "preserve_kitty_keyboard=true"
            } else {
                "preserve_kitty_keyboard=false"
            }),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// Write a bash script to `dir/name.sh`, mark it executable, and return
    /// its absolute path. Used to build fake-claude fixtures without a
    /// separate checked-in file.
    fn write_fake_claude(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(format!("{name}.sh"));
        fs::write(&path, body).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    /// Minimal env for a fake-claude shell script: just enough PATH for
    /// `/bin/sh` and basic utilities, plus a marker var the script can echo
    /// to prove env was passed through.
    fn minimal_env(extra: &[(&str, &str)]) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        for (k, v) in extra {
            env.insert(k.to_string(), v.to_string());
        }
        env
    }

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn spawns_and_waits_for_clean_exit() {
        let dir = TempDir::new().unwrap();
        let script = write_fake_claude(
            &dir,
            "clean_exit",
            "#!/bin/sh\necho hello from fake-claude\nexit 0\n",
        );

        let cfg = PtySpawnConfig {
            program: script.to_string_lossy().into_owned(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            env: minimal_env(&[]),
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        let mut session = PtySession::spawn(cfg).expect("spawn clean_exit");
        let status = session.wait().expect("wait clean_exit");
        assert!(status.success(), "expected clean exit, got {status:?}");
    }

    #[test]
    fn propagates_nonzero_exit_code() {
        let dir = TempDir::new().unwrap();
        let script = write_fake_claude(&dir, "crash", "#!/bin/sh\nexit 42\n");

        let cfg = PtySpawnConfig {
            program: script.to_string_lossy().into_owned(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            env: minimal_env(&[]),
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        let mut session = PtySession::spawn(cfg).expect("spawn crash");
        let status = session.wait().expect("wait crash");
        assert!(!status.success(), "expected non-success, got {status:?}");
    }

    #[test]
    fn env_is_not_inherited_from_parent() {
        // Seed the parent env with a var the caller did NOT include in cfg.
        // The child must not see it — this locks the "deterministic env"
        // invariant from the spec.
        let _env_guard = EnvGuard::set("AGENT_DOC_PTY_PARENT_LEAK", "leaked");

        let dir = TempDir::new().unwrap();
        let script = write_fake_claude(
            &dir,
            "env_check",
            "#!/bin/sh\nif [ -n \"${AGENT_DOC_PTY_PARENT_LEAK:-}\" ]; then exit 99; fi\nexit 0\n",
        );

        let cfg = PtySpawnConfig {
            program: script.to_string_lossy().into_owned(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            env: minimal_env(&[]),
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        let mut session = PtySession::spawn(cfg).expect("spawn env_check");
        let status = session.wait().expect("wait env_check");
        assert!(
            status.success(),
            "child saw parent env leak (expected clean env), got {status:?}"
        );
    }

    #[test]
    fn cwd_is_set_on_child() {
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("nested");
        fs::create_dir(&subdir).unwrap();
        let marker = subdir.join("marker.txt");

        // Fake-claude touches a marker file in cwd; if cwd is wrong the file
        // lands somewhere else and the assertion below fails.
        let script = write_fake_claude(&dir, "cwd_check", "#!/bin/sh\ntouch marker.txt\nexit 0\n");

        let cfg = PtySpawnConfig {
            program: script.to_string_lossy().into_owned(),
            args: vec![],
            cwd: subdir.clone(),
            env: minimal_env(&[]),
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        let mut session = PtySession::spawn(cfg).expect("spawn cwd_check");
        session.wait().expect("wait cwd_check");
        assert!(marker.exists(), "marker not created in requested cwd");
    }

    #[test]
    fn resize_after_spawn_succeeds() {
        let dir = TempDir::new().unwrap();
        // Long-ish sleep so the pty is still open when we resize.
        let script = write_fake_claude(&dir, "sleeper", "#!/bin/sh\nsleep 0.3\nexit 0\n");

        let cfg = PtySpawnConfig {
            program: script.to_string_lossy().into_owned(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            env: minimal_env(&[]),
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        let mut session = PtySession::spawn(cfg).expect("spawn sleeper");
        session
            .resize(PtySize {
                rows: 50,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize mid-run");
        session.wait().expect("wait sleeper");
    }

    #[test]
    fn missing_program_errors_cleanly() {
        let dir = TempDir::new().unwrap();
        let cfg = PtySpawnConfig {
            program: "/nonexistent/path/to/definitely-not-claude".to_string(),
            args: vec![],
            cwd: dir.path().to_path_buf(),
            env: minimal_env(&[]),
            size: PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            },
        };

        let err = match PtySession::spawn(cfg) {
            Ok(_) => panic!("spawn should fail for missing program"),
            Err(e) => e,
        };
        let msg = format!("{err:#}");
        assert!(
            msg.contains("spawn_command failed") || msg.contains("definitely-not-claude"),
            "error should identify the failed program, got: {msg}"
        );
    }

    // ---- #ctlrecycle R3: execve-adopt primitives (raw-fd master + bare-PID child) ----

    #[test]
    fn raw_pid_child_try_wait_kill_wait_lifecycle() {
        // A real long-running child whose reaping is handed entirely to RawPidChild
        // (forget the std handle so nothing else calls waitpid on the same PID).
        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        std::mem::forget(child);

        let mut adopted = RawPidChild::new(pid);
        assert_eq!(adopted.process_id(), Some(pid));
        assert!(
            adopted
                .try_wait()
                .expect("try_wait while running")
                .is_none(),
            "running child should not report an exit status yet"
        );

        adopted.kill().expect("kill adopted child");
        let status = adopted.wait().expect("wait reaps the killed child");
        assert!(
            !status.success(),
            "SIGHUP-killed child should be non-success, got {status:?}"
        );
        // Cached terminal status is stable across repeated polls after reaping.
        assert!(
            adopted.try_wait().expect("try_wait after exit").is_some(),
            "terminal status should be cached, not re-waitpid'd into ECHILD"
        );
    }

    #[test]
    fn raw_pid_kill_of_dead_process_is_ok() {
        // ESRCH (no such process) is treated as success, matching the PID-kill path.
        let child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id();
        std::mem::forget(child);
        let mut reaper = RawPidChild::new(pid);
        // Reap it so the PID is gone, then a kill must still be Ok (ESRCH).
        let _ = reaper.wait().expect("wait true");
        assert!(
            raw_pid_kill(pid).is_ok(),
            "kill of a reaped pid should be Ok"
        );
    }

    #[test]
    fn adopted_master_resize_and_size_round_trip() {
        let pair = NativePtySystem::default()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let raw = pair.master.as_raw_fd().expect("master raw fd");
        let duped = unsafe { libc::dup(raw) };
        assert!(duped >= 0, "dup master fd");

        let master = unsafe { AdoptedMaster::from_raw_fd(duped) };
        master
            .resize(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize adopted master");
        let size = master.get_size().expect("get_size");
        assert_eq!(size.rows, 30);
        assert_eq!(size.cols, 100);
        assert_eq!(master.as_raw_fd(), Some(duped));
        // Reader/writer handles dup the fd independently, so both succeed.
        let _reader = master.try_clone_reader().expect("clone reader");
        let _writer = master.take_writer().expect("take writer");
        drop(pair); // keep the original pty pair alive through the assertions
    }

    #[test]
    fn pty_session_adopt_drives_pid_and_master_plumbing() {
        let pair = NativePtySystem::default()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let raw = pair.master.as_raw_fd().expect("master raw fd");
        let duped = unsafe { libc::dup(raw) };

        let child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        std::mem::forget(child);

        let mut session = PtySession::adopt(duped, pid).expect("adopt session");
        assert_eq!(session.process_id(), Some(pid));
        // The master plumbing the supervisor relies on works through the adopted fd.
        let _reader = session.clone_reader().expect("clone_reader");
        let handle = session.resize_handle().expect("resize_handle");
        handle
            .resize(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("resize via handle");

        session.kill().expect("kill adopted child");
        let status = session.wait().expect("wait adopted child");
        assert!(!status.success(), "killed child should be non-success");
        drop(pair);
    }

    #[test]
    fn pty_session_adopt_rejects_invalid_inputs() {
        assert!(PtySession::adopt(-1, 123).is_err(), "negative fd rejected");
        assert!(PtySession::adopt(3, 0).is_err(), "zero pid rejected");
    }
}

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

use anyhow::{Context, Result};
use portable_pty::{
    Child, CommandBuilder, ExitStatus, MasterPty, NativePtySystem, PtySize, PtySystem,
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
    child: Box<dyn Child + Send + Sync>,
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
            child,
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
                            if crate::input_diag::verbose_enabled() {
                                crate::input_diag::log_byte_events(
                                    None,
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
        self.child.wait().context("child.wait failed")
    }

    /// Attempt to kill the child. Used by the IPC `stop` handler and on
    /// supervisor shutdown when the child refuses to exit via SIGHUP.
    #[allow(dead_code)] // API surface — used by tests; IPC stop uses libc::kill via PID
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().context("child.kill failed")
    }

    /// Create a thread-safe resize handle that can be sent to other threads
    /// (e.g., [`super::resize::ResizeWatcher`]).
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
        self.child.process_id()
    }
}

/// Stateful filter for terminal capability queries AND responses from pty output.
///
/// When a pty child (e.g. Claude Code) sends terminal queries (DSR, DA, XTVERSION),
/// and the supervisor forwards pty output to an outer terminal (tmux pane), the
/// outer terminal responds with escape sequences that echo as visible garbage
/// (e.g. `^[[?997;1n^[P>|tmux 3.6a^[\`).
///
/// **Strategy: suppress both queries and responses.** Stripping outgoing queries
/// prevents the outer terminal from generating responses in the first place.
/// Stripping responses is defense-in-depth for any that leak through.
///
/// This filter strips by default:
///
/// **Outgoing queries (from child → outer terminal):**
/// - CSI `c` with no prefix (DA1 query: `\x1b[c` or `\x1b[0c`)
/// - CSI `>` sequences ending in `q` (XTVERSION query: `\x1b[>q`)
///
/// **Incoming responses (echoed back through pty):**
/// - CSI `?` sequences ending in `n` (DSR responses: `\x1b[?997;1n`)
/// - CSI `?` sequences ending in `c` (DA1 responses: `\x1b[?1;2;4c`)
/// - CSI `?` sequences ending in `h`/`l` (DEC private mode set/reset: `\x1b[?2026h`)
/// - CSI `>` sequences ending in `c` (DA2 query/response: `\x1b[>0;115;0c`)
/// - CSI `>` sequences ending in `u` (Kitty keyboard push: `\x1b[>1u`)
/// - CSI `>` sequences ending in `m` (Kitty progressive enhancement: `\x1b[>4;2m`)
/// - CSI `<` sequences ending in `u` (Kitty keyboard pop: `\x1b[<u`)
/// - DCS strings (`\x1b P....\x1b\` — e.g. `\x1bP>|tmux 3.6a\x1b\`)
/// - OSC strings (`\x1b]....\x07` or `\x1b]....\x1b\`), including title updates
///
/// OpenCode is the exception for Kitty keyboard sequences: OpenTUI depends on
/// those mode transitions for real arrow/tab key handling, so the OpenCode
/// supervisor preserves them while still suppressing unrelated terminal queries.
///
/// Normal escape sequences (SGR colors, cursor movement, etc.) pass through.
///
/// **Stateful across reads:** Partial escape sequences at read boundaries are
/// buffered internally and completed on the next `filter()` call. This prevents
/// escape sequence leaks when an ESC byte lands at the end of one read buffer.
pub(crate) struct PtyFilter {
    carryover: Vec<u8>,
    preserve_kitty_keyboard: bool,
}

impl PtyFilter {
    #[allow(dead_code)] // default constructor used by filter unit-test wrappers
    pub(crate) fn new() -> Self {
        Self {
            carryover: Vec::new(),
            preserve_kitty_keyboard: false,
        }
    }

    pub(crate) fn for_harness(harness: &crate::harness::HarnessConfig) -> Self {
        Self {
            carryover: Vec::new(),
            preserve_kitty_keyboard: harness.binary == "opencode",
        }
    }

    pub(crate) fn filter(&mut self, input: &[u8], output: &mut Vec<u8>) {
        // Prepend any carryover from the previous call
        let combined;
        let data: &[u8] = if self.carryover.is_empty() {
            input
        } else {
            combined = [self.carryover.as_slice(), input].concat();
            self.carryover.clear();
            &combined
        };

        let len = data.len();
        let mut i = 0;
        while i < len {
            if data[i] == 0x1b {
                if i + 1 >= len {
                    // ESC at end of buffer — save for next call
                    self.carryover.extend_from_slice(&data[i..]);
                    return;
                }
                // ESC [ — CSI sequence
                if data[i + 1] == b'[' {
                    let start = i;
                    i += 2; // skip ESC [
                    if i >= len {
                        self.carryover.extend_from_slice(&data[start..]);
                        return;
                    }
                    // Check for ?, >, < prefix
                    let has_question = data[i] == b'?';
                    let has_gt = data[i] == b'>';
                    let has_lt = data[i] == b'<';
                    let no_prefix = !has_question && !has_gt && !has_lt;
                    if has_question || has_gt || has_lt {
                        i += 1; // skip prefix
                    }
                    // Consume parameter bytes (digits, semicolons)
                    while i < len && (data[i].is_ascii_digit() || data[i] == b';') {
                        i += 1;
                    }
                    if i >= len {
                        // Incomplete CSI — save for next call
                        self.carryover.extend_from_slice(&data[start..]);
                        return;
                    }
                    // Check final byte
                    if data[i].is_ascii_alphabetic() {
                        let final_byte = data[i];
                        i += 1; // consume final byte
                        let should_filter = match final_byte {
                            // CSI c with no prefix — DA1 query (\x1b[c or \x1b[0c)
                            b'c' if no_prefix => true,
                            // CSI ? ... n (DSR), c (DA1 response), h/l (DEC private mode set/reset)
                            b'n' | b'c' | b'h' | b'l' if has_question => true,
                            // CSI > ... c (DA2), q (XTVERSION)
                            b'c' | b'q' if has_gt => true,
                            // CSI > ... u (Kitty push), m (Kitty progressive)
                            b'u' | b'm' if has_gt => !self.preserve_kitty_keyboard,
                            // CSI < ... u (Kitty keyboard pop)
                            b'u' if has_lt => !self.preserve_kitty_keyboard,
                            _ => false,
                        };
                        if has_gt && matches!(final_byte, b'u' | b'm') {
                            let key = if final_byte == b'u' {
                                "kitty_keyboard_push"
                            } else {
                                "kitty_progressive_enhancement"
                            };
                            crate::input_diag::log_key_event(
                                None,
                                "supervisor.pty_filter",
                                "stdout",
                                if should_filter {
                                    "kitty_keyboard_drop"
                                } else {
                                    "kitty_keyboard_preserve"
                                },
                                key,
                                i - start,
                                crate::input_diag::KeyEventMeta {
                                    harness: None,
                                    detail: Some(if self.preserve_kitty_keyboard {
                                        "preserve_kitty_keyboard=true"
                                    } else {
                                        "preserve_kitty_keyboard=false"
                                    }),
                                },
                            );
                        } else if has_lt && final_byte == b'u' {
                            crate::input_diag::log_key_event(
                                None,
                                "supervisor.pty_filter",
                                "stdout",
                                if should_filter {
                                    "kitty_keyboard_drop"
                                } else {
                                    "kitty_keyboard_preserve"
                                },
                                "kitty_keyboard_pop",
                                i - start,
                                crate::input_diag::KeyEventMeta {
                                    harness: None,
                                    detail: Some(if self.preserve_kitty_keyboard {
                                        "preserve_kitty_keyboard=true"
                                    } else {
                                        "preserve_kitty_keyboard=false"
                                    }),
                                },
                            );
                        }
                        if should_filter {
                            continue; // drop this sequence
                        }
                        // Not filtered — emit the whole sequence
                        output.extend_from_slice(&data[start..i]);
                        continue;
                    }
                    // Malformed CSI — emit as-is
                    output.extend_from_slice(&data[start..i]);
                    continue;
                }
                // ESC P — DCS string (terminated by ESC \)
                if data[i + 1] == b'P' {
                    let start = i;
                    i += 2; // skip ESC P
                    // Scan for ESC \ (ST — String Terminator)
                    loop {
                        if i >= len {
                            // Incomplete DCS — save for next call
                            self.carryover.extend_from_slice(&data[start..]);
                            return;
                        }
                        if data[i] == 0x1b {
                            if i + 1 >= len {
                                // ESC at end inside DCS — save from DCS start
                                self.carryover.extend_from_slice(&data[start..]);
                                return;
                            }
                            if data[i + 1] == b'\\' {
                                i += 2; // skip ESC \
                                break;
                            }
                        }
                        i += 1;
                    }
                    continue; // drop the entire DCS string
                }
                // ESC ] — OSC string (terminated by BEL or ESC \)
                if data[i + 1] == b']' {
                    let start = i;
                    i += 2; // skip ESC ]
                    loop {
                        if i >= len {
                            // Incomplete OSC — save from OSC start
                            self.carryover.extend_from_slice(&data[start..]);
                            return;
                        }
                        if data[i] == 0x07 {
                            i += 1; // skip BEL terminator
                            break;
                        }
                        if data[i] == 0x1b {
                            if i + 1 >= len {
                                self.carryover.extend_from_slice(&data[start..]);
                                return;
                            }
                            if data[i + 1] == b'\\' {
                                i += 2; // skip ST terminator
                                break;
                            }
                        }
                        i += 1;
                    }
                    continue; // drop the entire OSC string
                }
                // Unknown ESC sequence — pass ESC + next byte through
            }
            output.push(data[i]);
            i += 1;
        }
    }
}

/// Stateless convenience wrapper for single-buffer filtering (tests).
#[cfg(test)]
fn filter_terminal_queries(input: &[u8], output: &mut Vec<u8>) {
    PtyFilter::new().filter(input, output);
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

    // --- filter: outgoing query suppression ---

    #[test]
    fn filter_strips_da1_query() {
        // CSI c — DA1 query (no prefix, no params)
        let input = b"\x1b[c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA1 query should be stripped");
    }

    #[test]
    fn filter_strips_da1_query_with_param() {
        // CSI 0 c — DA1 query with explicit param
        let input = b"\x1b[0c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA1 query with param should be stripped");
    }

    #[test]
    fn filter_strips_xtversion_query() {
        // CSI > q — XTVERSION query
        let input = b"\x1b[>q";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "XTVERSION query should be stripped");
    }

    // --- filter: response stripping ---

    #[test]
    fn filter_strips_dsr_response() {
        let input = b"\x1b[?997;1n";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DSR response should be stripped");
    }

    #[test]
    fn filter_strips_da1_response() {
        let input = b"\x1b[?1;2;4c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA1 response should be stripped");
    }

    #[test]
    fn filter_strips_da2_response() {
        let input = b"\x1b[>0;115;0c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA2 response should be stripped");
    }

    #[test]
    fn filter_strips_dcs_string() {
        // DCS: ESC P >|tmux 3.6a ESC \
        let input = b"\x1bP>|tmux 3.6a\x1b\\";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DCS string should be stripped");
    }

    #[test]
    fn filter_strips_interleaved_sequences() {
        // Real-world: DSR + DCS + DA1 + banner text
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b[?997;1n");
        input.extend_from_slice(b"\x1bP>|tmux 3.6a\x1b\\");
        input.extend_from_slice(b"\x1b[?1;2;4c");
        input.extend_from_slice(" Claude Code v2.1.109".as_bytes());
        let mut out = Vec::new();
        filter_terminal_queries(&input, &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            " Claude Code v2.1.109",
            "only the banner text should remain"
        );
    }

    #[test]
    fn filter_strips_dec_private_mode_set() {
        // CSI ? 2026 h — synchronized output mode set
        let input = b"\x1b[?2026h";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DEC private mode set should be stripped");
    }

    #[test]
    fn filter_strips_dec_private_mode_reset() {
        // CSI ? 2026 l — synchronized output mode reset
        let input = b"\x1b[?2026l";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DEC private mode reset should be stripped");
    }

    #[test]
    fn filter_strips_kitty_keyboard_push() {
        // CSI > 1 u — Kitty keyboard push mode
        let input = b"\x1b[>1u";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "Kitty keyboard push should be stripped");
    }

    #[test]
    fn filter_strips_kitty_keyboard_pop() {
        // CSI < u — Kitty keyboard pop mode
        let input = b"\x1b[<u";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "Kitty keyboard pop should be stripped");
    }

    #[test]
    fn filter_strips_kitty_progressive_enhancement() {
        // CSI > 4;2 m — Kitty progressive enhancement
        let input = b"\x1b[>4;2m";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(
            out.is_empty(),
            "Kitty progressive enhancement should be stripped"
        );
    }

    #[test]
    fn filter_preserves_kitty_keyboard_for_opencode() {
        let mut filter = PtyFilter::for_harness(&crate::harness::HarnessConfig::opencode());
        let input = b"\x1b[>1u\x1b[>4;2mkeys\x1b[<u";
        let mut out = Vec::new();
        filter.filter(input, &mut out);
        assert_eq!(
            out, input,
            "OpenCode relies on Kitty keyboard mode for permission prompt keys"
        );
    }

    #[test]
    fn opencode_filter_still_strips_terminal_queries() {
        let mut filter = PtyFilter::for_harness(&crate::harness::HarnessConfig::opencode());
        let input = b"\x1b[?997;1n\x1b[>q\x1bP>|tmux 3.6a\x1b\\\x1b[>1u";
        let mut out = Vec::new();
        filter.filter(input, &mut out);
        assert_eq!(
            out,
            b"\x1b[>1u".to_vec(),
            "OpenCode preserves keyboard mode but not terminal query noise"
        );
    }

    #[test]
    fn filter_preserves_normal_csi() {
        // SGR (color) sequence should pass through
        let input = b"\x1b[32mhello\x1b[0m";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(out, input.to_vec(), "SGR sequences should be preserved");
    }

    #[test]
    fn filter_preserves_cursor_movement() {
        // CSI 4 A — cursor up (no prefix, should pass through)
        let input = b"\x1b[4A";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(out, input.to_vec(), "cursor movement should be preserved");
    }

    #[test]
    fn filter_preserves_plain_text() {
        let input = b"hello world\n";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(
            out,
            input.to_vec(),
            "plain text should pass through unchanged"
        );
    }

    // --- Cross-boundary stateful filter tests ---

    #[test]
    fn filter_stateful_esc_split_across_reads() {
        // ESC at end of read 1, rest of DCS + DA1 in read 2
        let mut f = PtyFilter::new();
        let mut out = Vec::new();

        // Read 1: banner text + ESC (start of DCS)
        f.filter(b"hello\x1b", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "hello",
            "text before ESC emitted, ESC buffered"
        );

        // Read 2: P>|tmux 3.6a ESC \ + DA1 + more text
        out.clear();
        f.filter(b"P>|tmux 3.6a\x1b\\\x1b[?1;2;4c world", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            " world",
            "DCS and DA1 stripped, trailing text preserved"
        );
    }

    #[test]
    fn filter_stateful_dcs_split_at_st() {
        // DCS body spans reads, ST (ESC \) arrives in read 2
        let mut f = PtyFilter::new();
        let mut out = Vec::new();

        // Read 1: full DCS start + partial body ending with ESC
        f.filter(b"\x1bP>|tmux 3.6a\x1b", &mut out);
        assert!(out.is_empty(), "incomplete DCS buffered, nothing emitted");

        // Read 2: \ (completes ST) + normal text
        out.clear();
        f.filter(b"\\done", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "done",
            "DCS consumed after ST completed across boundary"
        );
    }

    #[test]
    fn filter_strips_osc_title_updates() {
        let input = b"before\x1b]0;Working (3s - esc to interrupt)\x07after";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "beforeafter",
            "OSC title text should not enter prompt sampling"
        );
    }

    #[test]
    fn filter_stateful_osc_split_across_reads() {
        let mut f = PtyFilter::new();
        let mut out = Vec::new();

        f.filter(b"before\x1b]0;Working", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "before",
            "text before OSC emitted, incomplete OSC buffered"
        );

        out.clear();
        f.filter(b" (3s - esc to interrupt)\x1b\\after", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "after",
            "OSC title update stripped after ST completed across boundary"
        );
    }

    #[test]
    fn filter_stateful_csi_split_at_params() {
        // CSI ?997; split across reads
        let mut f = PtyFilter::new();
        let mut out = Vec::new();

        // Read 1: ESC [ ? 997 (no final byte yet)
        f.filter(b"\x1b[?997", &mut out);
        assert!(out.is_empty(), "incomplete CSI buffered");

        // Read 2: ;1n (completes DSR) + text
        out.clear();
        f.filter(b";1nok", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "ok",
            "DSR stripped after completion across boundary"
        );
    }

    #[test]
    fn filter_stateful_csi_split_at_bracket() {
        // ESC [ split across reads
        let mut f = PtyFilter::new();
        let mut out = Vec::new();

        // Read 1: text + ESC
        f.filter(b"a\x1b", &mut out);
        assert_eq!(String::from_utf8_lossy(&out), "a");

        // Read 2: [?2026h (DEC private mode set)
        out.clear();
        f.filter(b"[?2026h", &mut out);
        assert!(out.is_empty(), "DEC mode set stripped across boundary");
    }

    #[test]
    fn filter_stateful_real_world_banner() {
        // Simulate the exact real-world scenario: DSR + DCS + DA1 + banner
        // split across 3 reads at arbitrary points
        let mut f = PtyFilter::new();
        let full = b"\x1b[?997;1n\x1bP>|tmux 3.6a\x1b\\\x1b[?1;2;4c Claude Code v2.1.109";

        // Split at arbitrary points
        let mut out = Vec::new();
        f.filter(&full[..5], &mut out); // \x1b[?99
        f.filter(&full[5..12], &mut out); // 7;1n\x1bP>
        f.filter(&full[12..25], &mut out); // |tmux 3.6a\x1b
        f.filter(&full[25..], &mut out); // \\x1b[?1;2;4c Claude Code v2.1.109

        assert_eq!(
            String::from_utf8_lossy(&out),
            " Claude Code v2.1.109",
            "all escape sequences stripped despite arbitrary split points"
        );
    }

    #[test]
    fn filter_stateful_normal_esc_preserved_across_boundary() {
        // SGR color code split across reads should still pass through
        let mut f = PtyFilter::new();
        let mut out = Vec::new();

        // Read 1: text + ESC
        f.filter(b"hi\x1b", &mut out);
        assert_eq!(String::from_utf8_lossy(&out), "hi");

        // Read 2: [32m (SGR green — should pass through)
        out.clear();
        f.filter(b"[32mgreen\x1b[0m", &mut out);
        assert_eq!(
            out, b"\x1b[32mgreen\x1b[0m",
            "SGR preserved across boundary"
        );
    }
}

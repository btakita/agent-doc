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
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().context("child.kill failed")
    }
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
        // SAFETY: test-only, single-threaded within this test's scope.
        unsafe {
            std::env::set_var("AGENT_DOC_PTY_PARENT_LEAK", "leaked");
        }

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
        let script = write_fake_claude(
            &dir,
            "cwd_check",
            "#!/bin/sh\ntouch marker.txt\nexit 0\n",
        );

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
}

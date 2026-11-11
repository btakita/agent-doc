//! Recreate a tmux server socket that was removed while the server stayed up.
//!
//! This is a one-shot at the start/command boundary, not a lifecycle Effect.
//! `agent-doc start` is a process with no long-lived graph for the ambient
//! tmux socket, and this adapter already owns the tmux command. tmux itself
//! rebuilds the socket when the server receives `SIGUSR1` (`server_create_socket`
//! unlinks the path, binds a new socket, and swaps the listen fd). A directory
//! entry that points at the old listen fd does not accept `connect` on Linux,
//! so this path does not try to relink `/proc/<pid>/fd`.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;

/// Signal tmux handles by recreating its server socket. The numeric value is
/// `libc::SIGUSR1` (10 on Linux, 30 on macOS), never a hardcoded 10.
#[cfg(unix)]
pub const RECREATE_SIGNAL: i32 = libc::SIGUSR1;

const RESTORE_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmbientSocketRestore {
    /// `$TMUX` is unset. Nothing to restore.
    NotInTmux,
    /// The socket named by `$TMUX` already accepts a connection.
    AlreadyConnectable { socket: PathBuf, pid: u32 },
    /// `SIGUSR1` made the live server bind a new socket at this path.
    Recreated { socket: PathBuf, pid: u32 },
}

#[derive(Debug)]
pub enum RestoreError {
    ServerGone {
        socket: PathBuf,
        pid: u32,
    },
    NotTmux {
        socket: PathBuf,
        pid: u32,
    },
    Parent {
        socket: PathBuf,
        message: String,
    },
    Signal {
        socket: PathBuf,
        pid: u32,
        message: String,
    },
    Timeout {
        socket: PathBuf,
        pid: u32,
    },
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServerGone { socket, pid } => write!(
                f,
                "tmux pid {pid} is not running, so {} cannot be recreated. Start tmux, then run `agent-doc start` again",
                socket.display()
            ),
            Self::NotTmux { socket, pid } => write!(
                f,
                "pid {pid} from $TMUX is not a tmux server, so {} was left unchanged",
                socket.display()
            ),
            Self::Parent { socket, message } => write!(
                f,
                "could not prepare the directory for {}: {message}",
                socket.display()
            ),
            Self::Signal {
                socket,
                pid,
                message,
            } => write!(
                f,
                "could not send SIGUSR1 to tmux pid {pid} for {}: {message}. Run `agent-doc tmux restore-socket` from a shell inside that tmux session",
                socket.display()
            ),
            Self::Timeout { socket, pid } => write!(
                f,
                "sent SIGUSR1 to tmux pid {pid} but {} did not become connectable. The socket directory must exist and must not be world-accessible",
                socket.display()
            ),
        }
    }
}

impl std::error::Error for RestoreError {}

/// `$TMUX` is `<socket>,<pid>,<session>`. The socket path is everything before
/// the last two commas so a path that itself contains a comma still parses.
pub fn parse_tmux_endpoint(value: &str) -> Option<(PathBuf, u32)> {
    let (socket, _session) = value.rsplit_once(',')?;
    let (socket, pid) = socket.rsplit_once(',')?;
    let pid = pid.parse().ok()?;
    if socket.is_empty() {
        return None;
    }
    Some((PathBuf::from(socket), pid))
}

/// Restore the socket named by `$TMUX` when that server is still alive.
///
/// A connectable socket is left untouched. Parallel callers take an exclusive
/// lock, recheck, and send `SIGUSR1` at most once.
pub fn restore_ambient_tmux_socket() -> Result<AmbientSocketRestore, RestoreError> {
    let Some(value) = std::env::var_os("TMUX") else {
        return Ok(AmbientSocketRestore::NotInTmux);
    };
    let Some(value) = value.to_str() else {
        return Ok(AmbientSocketRestore::NotInTmux);
    };
    let Some((socket, pid)) = parse_tmux_endpoint(value) else {
        return Ok(AmbientSocketRestore::NotInTmux);
    };
    if !pid_is_alive(pid) {
        if socket_connectable(&socket) {
            return Ok(AmbientSocketRestore::AlreadyConnectable { socket, pid });
        }
        return Err(RestoreError::ServerGone { socket, pid });
    }
    if !pid_is_tmux(pid) {
        return Err(RestoreError::NotTmux { socket, pid });
    }
    let outcome = restore_locked(&socket, pid, socket_connectable, signal_usr1)?;
    if matches!(outcome, AmbientSocketRestore::Recreated { .. }) {
        eprintln!(
            "[agent-doc] recreated missing tmux socket {} (SIGUSR1 pid {pid})",
            socket.display()
        );
    }
    Ok(outcome)
}

pub(crate) fn restore_locked(
    socket: &Path,
    pid: u32,
    connectable: impl Fn(&Path) -> bool,
    signal: impl Fn(u32) -> io::Result<()>,
) -> Result<AmbientSocketRestore, RestoreError> {
    if connectable(socket) {
        return Ok(AmbientSocketRestore::AlreadyConnectable {
            socket: socket.to_path_buf(),
            pid,
        });
    }
    let parent = socket
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| RestoreError::Parent {
            socket: socket.to_path_buf(),
            message: "socket path has no parent directory".to_string(),
        })?;
    ensure_private_parent(parent).map_err(|error| RestoreError::Parent {
        socket: socket.to_path_buf(),
        message: error.to_string(),
    })?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(parent.join(".agent-doc-socket-restore.lock"))
        .map_err(|error| RestoreError::Parent {
            socket: socket.to_path_buf(),
            message: error.to_string(),
        })?;
    lock.lock_exclusive()
        .map_err(|error| RestoreError::Parent {
            socket: socket.to_path_buf(),
            message: error.to_string(),
        })?;
    if connectable(socket) {
        return Ok(AmbientSocketRestore::AlreadyConnectable {
            socket: socket.to_path_buf(),
            pid,
        });
    }
    signal(pid).map_err(|error| RestoreError::Signal {
        socket: socket.to_path_buf(),
        pid,
        message: error.to_string(),
    })?;
    let deadline = Instant::now() + RESTORE_WAIT;
    loop {
        if connectable(socket) {
            return Ok(AmbientSocketRestore::Recreated {
                socket: socket.to_path_buf(),
                pid,
            });
        }
        if Instant::now() >= deadline {
            return Err(RestoreError::Timeout {
                socket: socket.to_path_buf(),
                pid,
            });
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn ensure_private_parent(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn socket_connectable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(path).is_ok()
    }
    #[cfg(not(unix))]
    {
        path.exists()
    }
}

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(windows)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success())
    }
}

fn pid_is_tmux(pid: u32) -> bool {
    if let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
        return text.trim().starts_with("tmux");
    }
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|comm| {
            comm.split_whitespace()
                .any(|word| word.split(':').next().is_some_and(|name| name == "tmux"))
        })
}

fn signal_usr1(pid: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as libc::pid_t, RECREATE_SIGNAL) };
        if rc == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        // Native Win32 has no SIGUSR1. MSYS2/Cygwin/Git Bash tmux still recreates
        // its socket on SIGUSR1, delivered by that environment's POSIX `kill`.
        // `taskkill` would stop the server.
        let status = std::process::Command::new("kill")
            .args(["-USR1", &pid.to_string()])
            .status()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("POSIX `kill -USR1` is not on PATH ({error})"),
                )
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "kill -USR1 {pid} exited {status}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn parses_tmux_endpoint_with_a_comma_in_the_socket_path() {
        let (socket, pid) = parse_tmux_endpoint("/tmp/my,socket,42,0").unwrap();
        assert_eq!(socket, PathBuf::from("/tmp/my,socket"));
        assert_eq!(pid, 42);
    }

    #[test]
    fn rejects_a_tmux_value_without_a_pid() {
        assert!(parse_tmux_endpoint("/tmp/tmux-1000/default").is_none());
    }

    #[test]
    fn a_connectable_socket_is_not_signaled() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("default");
        let hits = AtomicUsize::new(0);
        let outcome = restore_locked(
            &socket,
            7,
            |_| true,
            |_| {
                hits.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            outcome,
            AmbientSocketRestore::AlreadyConnectable {
                socket: socket.clone(),
                pid: 7,
            }
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn parallel_callers_signal_once() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("default");
        let hits = Arc::new(AtomicUsize::new(0));
        let ready = Arc::new(AtomicBool::new(false));
        thread::scope(|scope| {
            for _ in 0..8 {
                let hits = Arc::clone(&hits);
                let ready = Arc::clone(&ready);
                let socket = socket.clone();
                scope.spawn(move || {
                    restore_locked(
                        &socket,
                        7,
                        |path| path == socket.as_path() && ready.load(Ordering::SeqCst),
                        |_| {
                            hits.fetch_add(1, Ordering::SeqCst);
                            ready.store(true, Ordering::SeqCst);
                            Ok(())
                        },
                    )
                    .unwrap();
                });
            }
        });
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_parent_is_created_mode_0700() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("tmux-1000");
        ensure_private_parent(&parent).unwrap();
        let mode = std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn recreate_signal_is_sigusr1() {
        assert_eq!(RECREATE_SIGNAL, libc::SIGUSR1);
    }
}

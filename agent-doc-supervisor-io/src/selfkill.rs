//! `#supkill` — supervisor self-kill request channel + PCP-driven timeout force-kill.
//!
//! The `start --route-owned` supervisor is the **parent** of the live harness child
//! (`agent-doc(sup) → claude(agent)`), so it cannot be force-killed from inside its
//! own session — that signal would also kill the caller. Two rungs:
//!
//! - **`#supkill-a` (graceful, idle-gated self-kill):** the supervisor polls a
//!   per-document sentinel each idle tick and, at a turn boundary, tears down its
//!   harness child and exits cleanly (no relaunch). Idle-gating means a *healthy*
//!   live turn is never interrupted.
//! - **`#supkill-b` (PCP-driven timeout force-kill):** an external driver (the
//!   project controller, or `agent-doc admin kill-supervisor <FILE>`) records the
//!   graceful request, waits a grace window, then force-kills the supervisor pid by
//!   verified `/proc` cmdline when it is still alive. This is the robust backstop
//!   for a *wedged* supervisor that never reaches its idle loop — exactly the
//!   failure mode that makes the controller's dead-in-prod M1 self-watchdog
//!   useless. The driver refuses to signal its own ancestor, so an in-session
//!   caller can never kill itself.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use agent_doc_fs::document_state_hash;
use agent_doc_fs::find_project_root;

const CONTROL_DIR: &str = ".agent-doc/control";
const SELFKILL_SUFFIX: &str = "supervisor-selfkill";

/// Env override for the grace window before the force-kill escalation (`#supkill-b`).
pub const SELFKILL_GRACE_SECS_ENV: &str = "AGENT_DOC_SUPERVISOR_SELFKILL_GRACE_SECS";
const DEFAULT_SELFKILL_GRACE_SECS: u64 = 10;

/// Resolve the grace window the driver waits for a graceful self-kill before it
/// force-kills the supervisor pid. Env override, clamped to a positive value.
pub fn selfkill_grace() -> Duration {
    let secs = std::env::var(SELFKILL_GRACE_SECS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_SELFKILL_GRACE_SECS);
    Duration::from_secs(secs)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Per-document sentinel path: `<project_root>/.agent-doc/control/<hash>.supervisor-selfkill`.
pub fn self_kill_sentinel_path(file: &Path) -> Result<PathBuf> {
    let hash = document_state_hash(file)?;
    let canonical = file.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root
        .join(CONTROL_DIR)
        .join(format!("{hash}.{SELFKILL_SUFFIX}")))
}

/// `#supkill-b` — record a graceful self-kill request. The sentinel body is the unix
/// epoch seconds it was requested, which the force-kill grace timer reads back.
pub fn request_self_kill(file: &Path) -> Result<()> {
    let path = self_kill_sentinel_path(file)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, now_secs().to_string())?;
    Ok(())
}

/// `#supkill-a` — is a graceful self-kill pending for this document? Cheap existence
/// check the supervisor idle loop runs each tick.
pub fn self_kill_requested(file: &Path) -> bool {
    self_kill_sentinel_path(file)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// The unix-epoch seconds a self-kill was requested, if any (for the grace timer).
pub fn self_kill_requested_at(file: &Path) -> Option<u64> {
    let path = self_kill_sentinel_path(file).ok()?;
    let body = std::fs::read_to_string(&path).ok()?;
    body.trim().parse::<u64>().ok()
}

/// Clear the request after the supervisor self-exits or is reaped. Best-effort —
/// an already-absent sentinel is success.
pub fn clear_self_kill_request(file: &Path) {
    if let Ok(path) = self_kill_sentinel_path(file) {
        let _ = std::fs::remove_file(path);
    }
}

/// `pid`'s parent pid from `/proc/<pid>/stat`. The `comm` field (2nd) may contain
/// spaces/parens, so split after the last `)`; ppid is the 2nd whitespace field
/// after that.
#[cfg(unix)]
fn read_ppid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().nth(1)?.parse().ok()
}

/// In-session safety: is `pid` this process or any of its ancestors? The driver must
/// never force-kill the supervisor that spawned it (that kills the caller); an
/// in-session operator is told to run from another pane / let the PCP drive it.
#[cfg(unix)]
fn pid_is_self_or_ancestor(pid: u32) -> bool {
    let mut cur = std::process::id();
    loop {
        if cur == pid {
            return true;
        }
        if cur <= 1 {
            return false;
        }
        match read_ppid(cur) {
            Some(ppid) if ppid != cur => cur = ppid,
            _ => return false,
        }
    }
}

/// Public wrapper around [`pid_is_self_or_ancestor`] for the dead-supervisor
/// cold-start safety gate (`#supdead-coldstart-fallback`): a recovery command
/// must not cold-start / replace the supervisor that is this caller's own pane or
/// ancestor (that would tear down the caller).
#[cfg(unix)]
pub fn pid_is_self_or_ancestor_pub(pid: u32) -> bool {
    pid_is_self_or_ancestor(pid)
}

#[cfg(not(unix))]
pub fn pid_is_self_or_ancestor_pub(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Supervisor process discovery lives in [`crate::process`] — it answers a
/// liveness question ("is there a supervisor at all?"), not a kill-path one.
/// Re-exported so existing kill-path callers keep working.
pub use crate::process::supervisor_pid_for_doc;

/// `#supkill-b` — verified force-kill of a supervisor pid: SIGTERM, brief wait, then
/// SIGKILL if still alive. Re-verifies the cmdline before *each* signal so a recycled
/// pid is never killed, and refuses self/ancestor pids. Returns `true` once the pid
/// is gone.
#[cfg(unix)]
pub fn force_kill_verified_supervisor_pid(target: &Path, pid: u32) -> bool {
    if pid == std::process::id() || pid_is_self_or_ancestor(pid) {
        return false;
    }
    if !crate::process::supervisor_pid_matches_doc(pid, target) {
        return false;
    }
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(750) {
        if !process_is_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if crate::process::supervisor_pid_matches_doc(pid, target) {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    !process_is_alive(pid)
}

#[cfg(not(unix))]
pub fn force_kill_verified_supervisor_pid(_target: &Path, _pid: u32) -> bool {
    false
}

/// Outcome of [`drive_supervisor_kill`], so the CLI / PCP can report what happened.
#[derive(Debug, PartialEq, Eq)]
pub enum SupervisorKillOutcome {
    /// No live `start --route-owned` supervisor found for the document.
    NoSupervisor,
    /// The target supervisor is this caller's own ancestor — refused (run from
    /// another pane or let the PCP drive it). Carries the offending pid.
    RefusedSelfAncestor(u32),
    /// Dry-run: a supervisor was found but nothing was signalled. Carries the pid.
    WouldKill(u32),
    /// The supervisor honored the graceful self-kill within the grace window.
    Graceful(u32),
    /// The supervisor was force-killed after the grace window elapsed.
    Forced(u32),
}

/// `#supkill-b` — the PCP-style driver: request a graceful self-kill, wait up to
/// `grace` for the supervisor to honor it, then force-kill the verified pid. Safe to
/// call from any process; refuses to kill the caller's own ancestor. `dry_run`
/// reports the target without writing the sentinel or signalling.
#[cfg(unix)]
pub fn drive_supervisor_kill(
    file: &Path,
    grace: Duration,
    dry_run: bool,
) -> Result<SupervisorKillOutcome> {
    let Some(pid) = supervisor_pid_for_doc(file) else {
        return Ok(SupervisorKillOutcome::NoSupervisor);
    };
    if pid == std::process::id() || pid_is_self_or_ancestor(pid) {
        return Ok(SupervisorKillOutcome::RefusedSelfAncestor(pid));
    }
    if dry_run {
        return Ok(SupervisorKillOutcome::WouldKill(pid));
    }
    request_self_kill(file)?;
    // Poll for the graceful self-kill (the supervisor's idle loop honors the
    // sentinel at its next turn boundary).
    let start = Instant::now();
    loop {
        let alive = process_is_alive(pid);
        if !alive {
            clear_self_kill_request(file);
            return Ok(SupervisorKillOutcome::Graceful(pid));
        }
        if agent_doc_supervisor::selfkill::supervisor_force_kill_decision(
            start.elapsed(),
            alive,
            grace,
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Still alive after the grace window — force-kill the verified pid.
    let killed = force_kill_verified_supervisor_pid(file, pid);
    clear_self_kill_request(file);
    if killed {
        Ok(SupervisorKillOutcome::Forced(pid))
    } else if !process_is_alive(pid) {
        Ok(SupervisorKillOutcome::Graceful(pid))
    } else {
        // Verification refused the kill (pid recycled to a non-supervisor) — treat as
        // gone-enough for the caller; the document no longer has a wedged supervisor.
        Ok(SupervisorKillOutcome::Forced(pid))
    }
}

#[cfg(not(unix))]
pub fn drive_supervisor_kill(
    _file: &Path,
    _grace: Duration,
    _dry_run: bool,
) -> Result<SupervisorKillOutcome> {
    Ok(SupervisorKillOutcome::NoSupervisor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn never_kills_self_or_ancestor() {
        // This test process is trivially its own ancestor; the driver must refuse.
        assert!(pid_is_self_or_ancestor(std::process::id()));
        // pid 1 (init) is an ancestor of every process.
        assert!(pid_is_self_or_ancestor(1));
        // force-kill refuses self outright (never signals).
        let tmp = std::env::temp_dir().join("supkill-self-guard.md");
        assert!(!force_kill_verified_supervisor_pid(
            &tmp,
            std::process::id()
        ));
    }
}

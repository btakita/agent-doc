//! Reap detached daemon children spawned by a long-lived process (`#zombiereap`).
//!
//! `Command::spawn()` returns a [`std::process::Child`]; **dropping the handle
//! does not reap the OS process**. When a long-lived process (the route-owned
//! supervisor) launches a detached daemon, such as `agent-doc controller serve`
//! or `agent-doc watch`, and drops the `Child`, a fast-exiting child turns into
//! an unreaped `<defunct>` zombie parented to the launcher for the launcher's
//! whole life.
//!
//! Reap the handle on a detached thread instead of dropping it. `wait()` never
//! kills the child and never blocks the launcher: it only collects the exit
//! status once the child exits, so the daemon still runs independently.

/// Reap a spawned detached daemon child in the background so it can never become
/// a `<defunct>` zombie under a long-lived launcher. Prefer this over dropping
/// the [`std::process::Child`] returned by a fire-and-forget `spawn()`.
pub fn reap_detached(child: std::process::Child) {
    let _ = spawn_reaper(child);
}

/// Reap already-exited `agent-doc` children whose dedicated reaper thread was
/// lost across an in-place supervisor `execve` upgrade.
///
/// A live harness child is never touched: candidates must already be zombies
/// and their Linux `comm` must be exactly `agent-doc`.
#[cfg(target_os = "linux")]
pub fn reap_historical_agent_doc_zombies() -> usize {
    let children_path = format!("/proc/self/task/{}/children", std::process::id());
    let Ok(children) = std::fs::read_to_string(children_path) else {
        return 0;
    };
    let mut reaped = 0;
    for raw_pid in children.split_whitespace() {
        let Ok(pid) = raw_pid.parse::<libc::pid_t>() else {
            continue;
        };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        if !is_agent_doc_zombie(&comm, &stat) {
            continue;
        }
        let mut status = 0;
        let waited = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if waited == pid {
            reaped += 1;
        }
    }
    reaped
}

#[cfg(target_os = "linux")]
fn is_agent_doc_zombie(comm: &str, stat: &str) -> bool {
    let state = stat
        .rsplit_once(") ")
        .and_then(|(_, suffix)| suffix.chars().next());
    comm.trim() == "agent-doc" && state == Some('Z')
}

#[cfg(not(target_os = "linux"))]
pub fn reap_historical_agent_doc_zombies() -> usize {
    0
}

/// Spawn the reaper thread. Returns the [`JoinHandle`] so tests can
/// deterministically observe that the child was reaped; production callers use
/// [`reap_detached`] and ignore it. Closing the inherited stdio handles first
/// keeps an unread pipe from filling and blocking the daemon; this adapter only
/// cares about the exit status.
fn spawn_reaper(mut child: std::process::Child) -> Option<std::thread::JoinHandle<()>> {
    drop(child.stdin.take());
    drop(child.stdout.take());
    drop(child.stderr.take());
    let pid = child.id();
    std::thread::Builder::new()
        .name(format!("reap-detached-{pid}"))
        .spawn(move || {
            let _ = child.wait();
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn historical_reaper_filters_to_exited_agent_doc_children() {
        assert!(is_agent_doc_zombie("agent-doc\n", "42 (agent-doc) Z 1 2 3"));
        assert!(!is_agent_doc_zombie(
            "agent-doc\n",
            "42 (agent-doc) S 1 2 3"
        ));
        assert!(!is_agent_doc_zombie("claude\n", "42 (claude) Z 1 2 3"));
    }
    use std::process::{Command, Stdio};

    /// A child that has already exited by the time the reaper runs is still a
    /// zombie until someone `wait()`s it. Joining the reaper thread proves
    /// `wait()` returned.
    #[test]
    fn reap_detached_collects_a_fast_exiting_child() {
        let child = Command::new("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn `true`");
        let handle = spawn_reaper(child).expect("reaper thread spawns");
        handle.join().expect("reaper thread reaps and exits");
    }

    /// The reaper must also handle a child that is still running when it starts:
    /// `wait()` blocks until exit, then reaps.
    #[test]
    fn reap_detached_waits_for_a_still_running_child() {
        let child = Command::new("sleep")
            .arg("0.2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn `sleep`");
        let handle = spawn_reaper(child).expect("reaper thread spawns");
        handle.join().expect("reaper thread reaps and exits");
    }
}

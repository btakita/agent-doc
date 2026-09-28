//! Detect and recover a Codex TUI that is attached to the shared app-server
//! daemon.
//!
//! Codex 0.158 attaches every TUI to one shared `codex app-server` daemon, and
//! the daemon — not the TUI — spawns hooks and tool commands. They inherit the
//! daemon's environment, so `TMUX_PANE` names whichever pane happened to start
//! the daemon, and pane execution authority refuses a document's own owner as a
//! foreign pane. New launches run with `--no-daemon`; this module recovers the
//! sessions that were launched before that, or by hand.

use std::path::Path;

/// Whether an argv is the shared Codex app-server daemon.
pub fn argv_is_codex_app_server_daemon(argv: &[String]) -> bool {
    argv.iter().any(|arg| arg.contains("app-server-daemon"))
        || (argv.iter().any(|arg| is_codex_program(arg))
            && argv.iter().any(|arg| arg == "app-server"))
}

/// Whether an argv is an interactive Codex TUI that attaches to the shared
/// daemon: a `codex` launcher that is neither a daemon/app-server nor `exec`,
/// and was not started with `--no-daemon`.
pub fn argv_is_daemon_attached_codex_tui(argv: &[String]) -> bool {
    let Some(program_index) = argv.iter().position(|arg| is_codex_program(arg)) else {
        return false;
    };
    let rest = &argv[program_index + 1..];
    if argv_is_codex_app_server_daemon(argv) {
        return false;
    }
    if rest.first().is_some_and(|sub| {
        matches!(
            sub.as_str(),
            "exec" | "app-server" | "exec-server" | "mcp" | "mcp-server" | "login" | "logout"
        )
    }) {
        return false;
    }
    !rest.iter().any(|arg| arg == "--no-daemon")
}

fn is_codex_program(arg: &str) -> bool {
    Path::new(arg)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "codex")
}

#[cfg(target_os = "linux")]
fn read_argv(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        raw.split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect(),
    )
}

#[cfg(target_os = "linux")]
fn parent_pid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` may contain spaces and parens; fields resume after the LAST ')'.
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(1)?.parse().ok()
}

/// PID of the shared Codex app-server daemon this process descends from, if
/// any. When present, `TMUX_PANE` is the daemon's pane, not the invoker's.
#[cfg(target_os = "linux")]
pub fn invocation_descends_from_shared_codex_daemon() -> Option<u32> {
    let mut pid = std::process::id();
    for _ in 0..64 {
        pid = parent_pid(pid)?;
        if pid <= 1 {
            return None;
        }
        if read_argv(pid).is_some_and(|argv| argv_is_codex_app_server_daemon(&argv)) {
            return Some(pid);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn invocation_descends_from_shared_codex_daemon() -> Option<u32> {
    None
}

/// PID of a daemon-attached Codex TUI running in `pane_pid`'s process tree.
#[cfg(target_os = "linux")]
pub fn daemon_attached_codex_tui_under(pane_pid: u32) -> Option<u32> {
    let mut children: std::collections::HashMap<u32, Vec<u32>> = Default::default();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if let Some(ppid) = parent_pid(pid) {
            children.entry(ppid).or_default().push(pid);
        }
    }
    let mut stack = vec![pane_pid];
    let mut visited = 0usize;
    while let Some(pid) = stack.pop() {
        visited += 1;
        if visited > 4096 {
            return None;
        }
        if read_argv(pid).is_some_and(|argv| argv_is_daemon_attached_codex_tui(&argv)) {
            return Some(pid);
        }
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn daemon_attached_codex_tui_under(_pane_pid: u32) -> Option<u32> {
    None
}

/// Refusal text for a live-owner mismatch that is a shared-daemon artifact.
pub fn shared_daemon_owner_mismatch_remedy(
    document: &str,
    owner_pane_id: &str,
    reported_pane_id: &str,
    daemon_pid: u32,
    relaunch_requested: Result<(), String>,
) -> String {
    let recovery = match relaunch_requested {
        Ok(()) => format!(
            "agent-doc has requested the owner's Codex relaunch on its own app-server \
             (exact resume, conversation preserved) at this turn's boundary. When pane \
             {owner_pane_id} is back at its prompt, run `agent-doc {document}` there again."
        ),
        Err(err) => format!(
            "agent-doc could not request the relaunch automatically ({err}). Recover with \
             `agent-doc session restart-agent {document}`, then run `agent-doc {document}` in \
             pane {owner_pane_id} again."
        ),
    };
    format!(
        "pane execution authority rejected before mutation: live owner pane {owner_pane_id}, \
         reported invocation pane {reported_pane_id}. The reported pane is not evidence: this \
         command descends from the shared Codex app-server daemon (pid {daemon_pid}), which \
         hands every Codex TUI's hooks and commands the TMUX_PANE of the pane that started it, \
         and the Codex TUI in pane {owner_pane_id} is attached to that daemon. {recovery} This \
         command did not open or repair a cycle."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn classifies_the_shapes_seen_live() {
        let daemon = argv(&[
            "/home/u/.codex/packages/app-server-daemon/releases/0.158.0-x86_64-unknown-linux-musl/bin/codex",
            "app-server",
            "--listen",
        ]);
        assert!(argv_is_codex_app_server_daemon(&daemon));
        assert!(!argv_is_daemon_attached_codex_tui(&daemon));

        let attached = argv(&[
            "/usr/sbin/node",
            "/home/u/.npm-global/bin/codex",
            "-s",
            "danger-full-access",
            "--add-dir",
            "/repo",
        ]);
        assert!(argv_is_daemon_attached_codex_tui(&attached));
        assert!(!argv_is_codex_app_server_daemon(&attached));

        let resumed_attached = argv(&["/home/u/.npm-global/bin/codex", "resume", "01a0-thread"]);
        assert!(argv_is_daemon_attached_codex_tui(&resumed_attached));

        let isolated = argv(&[
            "/usr/sbin/node",
            "/home/u/.npm-global/bin/codex",
            "resume",
            "01a0-thread",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
            "--no-daemon",
        ]);
        assert!(!argv_is_daemon_attached_codex_tui(&isolated));

        let exec = argv(&["/home/u/.npm-global/bin/codex", "exec", "--json"]);
        assert!(!argv_is_daemon_attached_codex_tui(&exec));
        assert!(!argv_is_daemon_attached_codex_tui(&argv(&["/usr/bin/zsh"])));
    }

    #[test]
    fn remedy_says_the_pane_is_not_evidence_and_names_the_recovery() {
        let requested = shared_daemon_owner_mismatch_remedy("/r/tasks/infra.md", "%12", "%3", 171804, Ok(()));
        assert!(requested.contains("live owner pane %12"));
        assert!(requested.contains("reported invocation pane %3"));
        assert!(requested.contains("pid 171804"));
        assert!(requested.contains("requested the owner's Codex relaunch"));
        assert!(requested.contains("`agent-doc /r/tasks/infra.md`"));

        let manual = shared_daemon_owner_mismatch_remedy(
            "/r/tasks/infra.md",
            "%12",
            "%3",
            171804,
            Err("controller unavailable".into()),
        );
        assert!(manual.contains("`agent-doc session restart-agent /r/tasks/infra.md`"));
        assert!(manual.contains("controller unavailable"));
    }
}

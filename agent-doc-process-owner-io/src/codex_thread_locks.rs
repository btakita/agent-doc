//! Name what else has a Codex conversation open (`#codexlockholder`).
//!
//! Codex serializes writers to a conversation with one lock file per thread
//! under `$CODEX_HOME/thread-writer-locks/<thread-id>.lock`. A second client that
//! cannot take that lock renders the TUI screen agent-doc already classifies as
//! [`agent_doc_harness::CODEX_CONVERSATION_OPEN_ELSEWHERE_BLOCKER`]:
//!
//! ```text
//! 🔒  This conversation is open in another app
//!     Close it there and press R to continue here.
//!    r retry   f fork   esc/ctrl+c/q exit   ctrl+t transcript
//! ```
//!
//! That screen names no holder, so the operator is told to close something
//! somewhere with no way to find it. Operator-reported 2026-09-28 after the
//! agent for `src/haiven-dev/tasks/api.md` was switched from claude to codex:
//! *"agent-doc should say what else has the conversation open."*
//!
//! The holder is observable: the lock is an advisory file lock, so the process
//! holding it has the lock file open, and `/proc/<pid>/fd` names it. The
//! distinction that matters to the operator is whether the holder is another
//! terminal they can close, or the background **app-server daemon** that backs
//! non-terminal Codex clients — closing a terminal never clears the latter.
//!
//! This is observation only. Nothing here takes, breaks, or waits on a lock.

use std::fs;
use std::path::{Path, PathBuf};

/// What kind of Codex client holds a conversation's writer lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLockHolderKind {
    /// The managed `codex app-server` daemon — the background service behind
    /// non-terminal clients (IDE extension, desktop). Not a pane to close.
    AppServerDaemon,
    /// An interactive `codex` process — another terminal or pane.
    Interactive,
    /// A process holding the lock that does not look like either. Reported as
    /// itself rather than guessed at.
    Unknown,
}

impl CodexLockHolderKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AppServerDaemon => "app_server_daemon",
            Self::Interactive => "interactive",
            Self::Unknown => "unknown",
        }
    }

    /// Operator-facing phrasing: what the holder is, in the terms that decide
    /// what they can do about it.
    pub const fn operator_label(self) -> &'static str {
        match self {
            Self::AppServerDaemon => {
                "the Codex app-server daemon (a background service behind non-terminal clients, not a pane you can close)"
            }
            Self::Interactive => "an interactive codex session",
            Self::Unknown => "an unrecognized process",
        }
    }
}

/// One live holder of one conversation's writer lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexThreadLockHolder {
    /// The Codex thread (conversation) id, from the lock file name.
    pub thread_id: String,
    pub pid: String,
    pub kind: CodexLockHolderKind,
    /// The holder's command line, trimmed for a log line.
    pub command: String,
}

/// Classify a holder from its command line.
///
/// Pure so the classification is assertable without a live Codex install: the
/// `/proc` walk that produces the command line is the part that needs one.
pub fn classify_codex_lock_holder_command(command: &str) -> CodexLockHolderKind {
    let lower = command.to_ascii_lowercase();
    // Match the EXECUTABLE, not the string "codex" anywhere in the line: a
    // backup or editor process whose arguments merely mention `~/.codex` holds
    // the lock file open too, and reporting it as a Codex session would send the
    // operator looking for a terminal that does not exist.
    let executable = lower
        .split_whitespace()
        .next()
        .and_then(|argv0| argv0.rsplit('/').next())
        .unwrap_or_default();
    if executable != "codex" {
        return CodexLockHolderKind::Unknown;
    }
    // `codex app-server ...` is the daemon in every observed form, including
    // `--managed-daemon` and the `daemon pid-update-loop` child.
    if lower.contains("app-server") || lower.contains("app_server") {
        return CodexLockHolderKind::AppServerDaemon;
    }
    CodexLockHolderKind::Interactive
}

/// `$CODEX_HOME/thread-writer-locks`, defaulting to `~/.codex`.
pub fn codex_thread_lock_dir() -> Option<PathBuf> {
    let home = match std::env::var_os("CODEX_HOME") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".codex"),
    };
    Some(home.join("thread-writer-locks"))
}

fn thread_id_from_lock_path(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    name.strip_suffix(".lock").map(str::to_string)
}

fn process_command_line(pid: &str) -> Option<String> {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let joined = String::from_utf8_lossy(&raw).replace('\0', " ");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(200).collect())
}

/// Every live holder of a Codex conversation writer lock, newest lock first.
///
/// Returns an empty vector when no Codex home, no lock directory, or no holder
/// is observable. "I could not look" and "nothing holds one" are reported the
/// same way on purpose: callers use this to *enrich* a blocker they already
/// classified from the pane, never to decide that one exists.
pub fn codex_thread_lock_holders() -> Vec<CodexThreadLockHolder> {
    let Some(dir) = codex_thread_lock_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut locks: Vec<(std::time::SystemTime, PathBuf, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let thread_id = thread_id_from_lock_path(&path)?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, fs::canonicalize(&path).unwrap_or(path), thread_id))
        })
        .collect();
    if locks.is_empty() {
        return Vec::new();
    }
    locks.sort_by_key(|lock| std::cmp::Reverse(lock.0));

    let mut holders = Vec::new();
    let Ok(procs) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let pids: Vec<String> = procs
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.chars().all(|c| c.is_ascii_digit()).then_some(name)
        })
        .collect();
    for pid in &pids {
        let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) else {
            // A process we may not inspect is not a holder we can name; skip it
            // rather than reporting an unknown holder for every lock.
            continue;
        };
        for fd in fds.filter_map(Result::ok) {
            let Ok(target) = fs::read_link(fd.path()) else {
                continue;
            };
            let Some((_, _, thread_id)) = locks.iter().find(|(_, path, _)| *path == target) else {
                continue;
            };
            let command = process_command_line(pid).unwrap_or_default();
            holders.push(CodexThreadLockHolder {
                thread_id: thread_id.clone(),
                pid: pid.clone(),
                kind: classify_codex_lock_holder_command(&command),
                command,
            });
        }
    }
    // Preserve lock recency: the conversation the operator just tried to open is
    // the most recently touched one.
    let order: Vec<&String> = locks.iter().map(|(_, _, id)| id).collect();
    holders.sort_by_key(|holder| {
        order
            .iter()
            .position(|id| **id == holder.thread_id)
            .unwrap_or(usize::MAX)
    });
    holders
}

/// One operator-facing line naming who has a Codex conversation open.
///
/// `None` when nothing could be observed — the caller then reports the blocker
/// exactly as it did before, never a claim that no holder exists.
pub fn describe_codex_thread_lock_holders(holders: &[CodexThreadLockHolder]) -> Option<String> {
    if holders.is_empty() {
        return None;
    }
    let rendered: Vec<String> = holders
        .iter()
        .take(3)
        .map(|holder| {
            format!(
                "thread {} held by pid {} — {}",
                holder.thread_id,
                holder.pid,
                holder.kind.operator_label()
            )
        })
        .collect();
    let mut line = rendered.join("; ");
    if holders.len() > 3 {
        line.push_str(&format!(" (+{} more)", holders.len() - 3));
    }
    Some(line)
}

/// The same facts as a single `key=value` ops.log fragment.
pub fn codex_thread_lock_holder_log_fields(holders: &[CodexThreadLockHolder]) -> String {
    if holders.is_empty() {
        return "lock_holders=unobserved".to_string();
    }
    let rendered: Vec<String> = holders
        .iter()
        .take(3)
        .map(|holder| {
            format!(
                "{}:{}:{}",
                holder.thread_id,
                holder.pid,
                holder.kind.as_str()
            )
        })
        .collect();
    format!("lock_holders={}", rendered.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction the operator acts on: a terminal they can close versus a
    /// background service that closing terminals never clears. Observed
    /// 2026-09-28 — seven interactive holders and three held by one
    /// `codex app-server --managed-daemon`, and the conversation the TUI refused
    /// was one of the daemon's.
    #[test]
    fn a_daemon_holder_is_not_reported_as_a_closable_session() {
        assert_eq!(
            classify_codex_lock_holder_command(
                "/home/u/.codex/packages/app-server-daemon/releases/0.158.0-x86_64-unknown-linux-musl/bin/codex app-server --listen unix:// --managed-daemon"
            ),
            CodexLockHolderKind::AppServerDaemon
        );
        assert_eq!(
            classify_codex_lock_holder_command(
                "/home/u/.codex/packages/app-server-daemon/releases/0.158.0/bin/codex app-server daemon pid-update-loop"
            ),
            CodexLockHolderKind::AppServerDaemon
        );
        assert_eq!(
            classify_codex_lock_holder_command(
                "/home/u/.npm-global/lib/node_modules/@openai/codex/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/codex/codex"
            ),
            CodexLockHolderKind::Interactive
        );
        // Never guess: a holder that is not a Codex process at all is reported
        // as itself, not as a session the operator should go close.
        assert_eq!(
            classify_codex_lock_holder_command("/usr/bin/rsync /home/u/.codex /backup"),
            CodexLockHolderKind::Unknown
        );
        assert_eq!(
            classify_codex_lock_holder_command(""),
            CodexLockHolderKind::Unknown
        );
    }

    #[test]
    fn an_unobserved_holder_set_never_claims_the_conversation_is_free() {
        assert_eq!(describe_codex_thread_lock_holders(&[]), None);
        assert_eq!(
            codex_thread_lock_holder_log_fields(&[]),
            "lock_holders=unobserved"
        );
    }

    #[test]
    fn a_described_holder_names_the_thread_the_pid_and_what_it_is() {
        let holders = vec![
            CodexThreadLockHolder {
                thread_id: "01a0e8a9-8c34-7371-abe8-8f70c3400366".to_string(),
                pid: "761544".to_string(),
                kind: CodexLockHolderKind::AppServerDaemon,
                command: "codex app-server --managed-daemon".to_string(),
            },
            CodexThreadLockHolder {
                thread_id: "01a0d430-42a0-7df2-8dc5-5b69017a7629".to_string(),
                pid: "2060074".to_string(),
                kind: CodexLockHolderKind::Interactive,
                command: "codex".to_string(),
            },
        ];
        let described = describe_codex_thread_lock_holders(&holders).expect("holders describe");
        assert!(described.contains("01a0e8a9-8c34-7371-abe8-8f70c3400366"));
        assert!(described.contains("pid 761544"));
        assert!(
            described.contains("app-server daemon"),
            "the operator must be told closing a terminal will not clear it: {described}"
        );
        assert_eq!(
            codex_thread_lock_holder_log_fields(&holders),
            "lock_holders=01a0e8a9-8c34-7371-abe8-8f70c3400366:761544:app_server_daemon,01a0d430-42a0-7df2-8dc5-5b69017a7629:2060074:interactive"
        );
    }

    #[test]
    fn the_lock_directory_follows_codex_home() {
        // Read-only resolution; no lock is opened, taken, or removed.
        let dir = codex_thread_lock_dir().expect("HOME or CODEX_HOME resolves in test env");
        assert!(dir.ends_with("thread-writer-locks"), "{dir:?}");
    }

    #[test]
    fn a_lock_file_name_is_the_thread_id() {
        assert_eq!(
            thread_id_from_lock_path(Path::new("/x/thread-writer-locks/01a0e8a9-abc.lock")),
            Some("01a0e8a9-abc".to_string())
        );
        assert_eq!(
            thread_id_from_lock_path(Path::new("/x/thread-writer-locks/not-a-lock")),
            None
        );
    }
}

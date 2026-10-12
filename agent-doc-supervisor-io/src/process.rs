//! Supervisor process discovery by owned document.
//!
//! "Which live `start --route-owned` process supervises this document?" is a
//! **discovery** question, not a kill-path concern. It previously lived in
//! [`crate::selfkill`] because the verified force-kill was its first consumer,
//! but the kill path is only one caller: liveness checks use the same lookup to
//! answer "is there a supervisor at all?" — and that question decides whether a
//! `queue: go` document has an idle-queue watch to self-drain it.
//!
//! `selfkill` keeps what is genuinely kill-safety (`pid_is_self_or_ancestor`,
//! the SIGTERM/SIGKILL escalation) and calls into this module for lookup.

use std::path::{Path, PathBuf};

#[cfg(unix)]
fn read_proc_args(pid: u32) -> Option<Vec<String>> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = cmdline
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).to_string())
        .collect();
    if args.is_empty() { None } else { Some(args) }
}

/// Parent pid (field 4) from a `/proc/<pid>/stat` line, counted from the
/// last `)` because `comm` may contain spaces and parentheses.
fn parse_proc_stat_ppid(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

pub fn process_parent_pid(pid: u32) -> Option<u32> {
    parse_proc_stat_ppid(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

/// The pure fork-image rule over two raw `/proc/<pid>/cmdline` reads: a child
/// whose command line is its parent's, byte for byte, has not exec'd yet.
pub fn cmdline_is_unexeced_fork_image(own: &[u8], parent: &[u8]) -> bool {
    !own.is_empty() && own == parent
}

/// `pid` is a fork of its parent that has not exec'd yet: its command line is
/// the parent's, byte for byte. A shell that forks before every external
/// command (`/bin/sh` = dash, a `( ... )` subshell, a `vfork` child) briefly or
/// persistently shows the parent's argv, and so does a Rust `Command::spawn`
/// child between its fork and its exec, so a `/proc` scan matching command
/// lines sees the same process image twice (#reaperciflake,
/// #controllerforkimage). Discovery that counts or targets controllers and
/// supervisors skips such a pid; the parent is the real process.
pub fn is_unexeced_fork_of_parent(pid: u32) -> bool {
    let Some(ppid) = process_parent_pid(pid).filter(|ppid| *ppid > 0) else {
        return false;
    };
    let (Ok(own), Ok(parent)) = (
        std::fs::read(format!("/proc/{pid}/cmdline")),
        std::fs::read(format!("/proc/{ppid}/cmdline")),
    ) else {
        return false;
    };
    cmdline_is_unexeced_fork_image(&own, &parent)
}

/// Canonical owned-document path for a running supervisor pid, resolving a relative
/// cmdline path against the process's `/proc/<pid>/cwd`. `None` if `pid` is not a
/// `start --route-owned` supervisor or its path cannot be resolved.
#[cfg(unix)]
pub fn supervisor_doc_canonical(pid: u32) -> Option<PathBuf> {
    let args = read_proc_args(pid)?;
    let doc = agent_doc_supervisor::selfkill::start_route_owned_doc_from_args(&args)?;
    if doc.is_absolute() {
        doc.canonicalize().ok()
    } else {
        let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
        cwd.join(doc).canonicalize().ok()
    }
}

#[cfg(not(unix))]
pub fn supervisor_doc_canonical(_pid: u32) -> Option<PathBuf> {
    None
}

/// Does `pid` own `target` (same canonical document)? Used to verify a kill target
/// before signalling — never SIGKILL a pid whose cmdline is not the expected
/// supervisor for this exact document.
#[cfg(unix)]
pub fn supervisor_pid_matches_doc(pid: u32, target: &Path) -> bool {
    match (supervisor_doc_canonical(pid), target.canonicalize().ok()) {
        (Some(doc), Some(want)) => doc == want,
        _ => false,
    }
}

#[cfg(not(unix))]
pub fn supervisor_pid_matches_doc(_pid: u32, _target: &Path) -> bool {
    false
}

/// Scan `/proc` for the live `start --route-owned` supervisor owning `target`.
///
/// A supervisor's un-exec'd fork image (same command line, parent = the
/// supervisor) is the same supervisor, not a second one: returning its pid
/// would make a supervisor asking about its own document see a foreign owner,
/// read a pid change where there is none, and aim the verified kill at a
/// short-lived child instead of the supervisor (#controllerforkimage).
///
/// `None` means no supervisor process exists for the document — which also means
/// no idle-queue watch, so an active queue cannot self-drain.
#[cfg(unix)]
pub fn supervisor_pid_for_doc(target: &Path) -> Option<u32> {
    let entries = std::fs::read_dir("/proc").ok()?;
    let pids = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok());
    first_supervisor_pid(
        pids,
        std::process::id(),
        |pid| supervisor_pid_matches_doc(pid, target),
        is_unexeced_fork_of_parent,
    )
}

/// The selection behind [`supervisor_pid_for_doc`], over a scan order and two
/// probes: the first pid that is not `self_pid`, supervises the document, and
/// is not an un-exec'd fork image. `/proc` lists pids in no promised order, and
/// after pid wrap a fork image can precede its parent.
fn first_supervisor_pid(
    pids: impl IntoIterator<Item = u32>,
    self_pid: u32,
    supervises: impl Fn(u32) -> bool,
    is_fork_image: impl Fn(u32) -> bool,
) -> Option<u32> {
    pids.into_iter()
        .find(|pid| *pid != self_pid && supervises(*pid) && !is_fork_image(*pid))
}

#[cfg(not(unix))]
pub fn supervisor_pid_for_doc(_target: &Path) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_stat_ppid_parse_counts_fields_from_the_last_paren() {
        assert_eq!(
            parse_proc_stat_ppid("4242 (agent doc) x) S 17 4242 0"),
            Some(17)
        );
        assert_eq!(parse_proc_stat_ppid("garbage"), None);
    }

    #[test]
    fn fork_image_rule_needs_a_byte_identical_non_empty_cmdline() {
        let parent = b"agent-doc\0start\0--route-owned\0plan.md\0";
        assert!(cmdline_is_unexeced_fork_image(parent, parent));
        assert!(!cmdline_is_unexeced_fork_image(b"git\0status\0", parent));
        assert!(!cmdline_is_unexeced_fork_image(b"", b""));
    }

    /// A fake process tree in a wrapped pid order: supervisor 40's fork image
    /// 7 is scanned first. Discovery names the supervisor, never its image, and
    /// a supervisor asking about its own document sees no other owner.
    #[test]
    fn supervisor_selection_skips_a_fork_image_scanned_before_its_parent() {
        let supervises = |pid: u32| matches!(pid, 7 | 40);
        let is_fork_image = |pid: u32| pid == 7;
        assert_eq!(
            first_supervisor_pid([3, 7, 40], 1, supervises, is_fork_image),
            Some(40)
        );
        assert_eq!(
            first_supervisor_pid([3, 7, 40], 40, supervises, is_fork_image),
            None
        );
        assert_eq!(
            first_supervisor_pid([3, 7, 40], 1, supervises, |_| false),
            Some(7)
        );
    }

    #[test]
    fn a_fresh_exec_is_not_a_fork_image_of_its_parent() {
        assert!(!is_unexeced_fork_of_parent(std::process::id()));
    }

    /// A fake supervisor whose shell keeps a `( ... ) &` subshell that never
    /// execs: the subshell is the supervisor's fork image (same command line,
    /// parent = the shell), the persistent form of the window dash opens before
    /// every external command and `Command::spawn` opens before every exec.
    #[cfg(target_os = "linux")]
    #[test]
    fn supervisor_discovery_names_the_supervisor_not_its_fork_image() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};

        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();
        let mut supervisor = Command::new("sh")
            .arg("-c")
            .arg("(while :; do sleep 1; done) & wait")
            .arg("agent-doc")
            .arg("start")
            .arg("--route-owned")
            .arg(&doc)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let fork_image = loop {
            let fork_image = std::fs::read_dir("/proc")
                .unwrap()
                .flatten()
                .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
                .find(|pid| {
                    process_parent_pid(*pid) == Some(supervisor.id())
                        && supervisor_pid_matches_doc(*pid, &doc)
                });
            if let Some(fork_image) = fork_image {
                break Some(fork_image);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let observed = fork_image.map(|fork_image| {
            (
                is_unexeced_fork_of_parent(fork_image),
                is_unexeced_fork_of_parent(supervisor.id()),
                supervisor_pid_for_doc(&doc),
            )
        });
        if let Some(fork_image) = fork_image {
            let _ = Command::new("kill")
                .arg("-KILL")
                .arg(fork_image.to_string())
                .status();
        }
        let _ = supervisor.kill();
        let _ = supervisor.wait();

        let (image_is_fork, supervisor_is_fork, discovered) =
            observed.expect("fake supervisor never forked its subshell image");
        assert!(image_is_fork);
        assert!(!supervisor_is_fork);
        assert_eq!(discovered, Some(supervisor.id()));
    }

    /// Discovery must be total: an unknown document simply has no supervisor,
    /// never a panic or an error the caller has to special-case.
    #[test]
    fn an_unsupervised_document_resolves_to_no_pid() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();
        assert_eq!(supervisor_pid_for_doc(&doc), None);
    }

    /// This process is not a route-owned supervisor for an arbitrary document, so
    /// the cmdline verification must reject it rather than matching loosely.
    #[test]
    fn a_non_supervisor_pid_does_not_match_a_document() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();
        assert!(!supervisor_pid_matches_doc(std::process::id(), &doc));
    }
}

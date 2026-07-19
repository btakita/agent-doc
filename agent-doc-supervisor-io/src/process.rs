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
/// `None` means no supervisor process exists for the document — which also means
/// no idle-queue watch, so an active queue cannot self-drain.
#[cfg(unix)]
pub fn supervisor_pid_for_doc(target: &Path) -> Option<u32> {
    let self_pid = std::process::id();
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }
        if supervisor_pid_matches_doc(pid, target) {
            return Some(pid);
        }
    }
    None
}

#[cfg(not(unix))]
pub fn supervisor_pid_for_doc(_target: &Path) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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

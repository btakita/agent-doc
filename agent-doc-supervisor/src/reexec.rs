//! Pure supervisor reexec candidate ordering.

use std::path::PathBuf;

/// Ordered, de-duplicated candidate binary paths the supervisor self-`execve`
/// may target, each tagged with a short diagnostic note.
///
/// The caller gathers environment facts such as `current_exe()` and the freshly
/// resolved installed binary; this pure builder owns only the ladder policy.
/// The list always ends with the bare-name `PATH` fallback so it is never empty.
pub fn build_reexec_candidates(
    resolved_fresh: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    current_exe_launchable: bool,
) -> Vec<(PathBuf, &'static str)> {
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    let mut push = |path: PathBuf, note: &'static str| {
        if !out.iter().any(|(existing, _)| existing == &path) {
            out.push((path, note));
        }
    };
    if let Some(path) = resolved_fresh {
        push(path, "resolved_fresh_binary");
    }
    if current_exe_launchable && let Some(path) = current_exe {
        push(path, "current_exe");
    }
    // Bare `agent-doc` -> OS `PATH` lookup at exec time. Last-resort fallback so
    // a `PATH`-only install still recycles even if argv0/`current_exe` are gone.
    push(PathBuf::from("agent-doc"), "path_lookup_agent_doc");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexec_candidates_prefer_fresh_then_current_exe_then_path() {
        let fresh = PathBuf::from("/fresh/agent-doc");
        let current = PathBuf::from("/proc/self/exe-current");
        let candidates = build_reexec_candidates(Some(fresh.clone()), Some(current.clone()), true);
        let paths: Vec<_> = candidates.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(
            paths,
            vec![fresh, current, PathBuf::from("agent-doc")],
            "ordered: resolved fresh, launchable current_exe, then PATH fallback"
        );
    }

    #[test]
    fn reexec_candidates_drop_deleted_current_exe_but_keep_path_fallback() {
        // The Linux post-`cargo install` shape: `current_exe()` is a `(deleted)`
        // inode (not launchable) and the fresh resolver succeeded. The deleted
        // path must not appear; the PATH fallback always does so the ladder is
        // never empty.
        let fresh = PathBuf::from("/home/u/.cargo/bin/agent-doc");
        let deleted = PathBuf::from("/home/u/.cargo/bin/agent-doc (deleted)");
        let candidates = build_reexec_candidates(Some(fresh.clone()), Some(deleted.clone()), false);
        let notes: Vec<_> = candidates.iter().map(|(_, n)| *n).collect();
        assert_eq!(
            notes,
            vec!["resolved_fresh_binary", "path_lookup_agent_doc"]
        );
        assert!(
            !candidates.iter().any(|(p, _)| p == &deleted),
            "a non-launchable (deleted) current_exe must be excluded"
        );
    }

    #[test]
    fn reexec_candidates_dedup_and_never_empty() {
        // When the resolver and current_exe both point at the same path, it
        // appears once; with neither resolvable the PATH fallback alone keeps
        // the list usable.
        let same = PathBuf::from("/usr/local/bin/agent-doc");
        let deduped = build_reexec_candidates(Some(same.clone()), Some(same.clone()), true);
        assert_eq!(
            deduped,
            vec![
                (same, "resolved_fresh_binary"),
                (PathBuf::from("agent-doc"), "path_lookup_agent_doc"),
            ]
        );

        let empty = build_reexec_candidates(None, None, false);
        assert_eq!(
            empty,
            vec![(PathBuf::from("agent-doc"), "path_lookup_agent_doc")],
            "ladder always ends with a PATH fallback so reexec can still try"
        );
    }
}

//! Test runtime scope: which live runtimes a test process may observe or drive
//! (`#testisolationleaks`).
//!
//! `#testisolationtests` stops a test from *adopting* a real project as its
//! root by walking up from its tempdir. It does not stop a test from being
//! *handed* a real root. Process discovery (`/proc` walks for `controller
//! serve` and `start` command lines) names every live controller and
//! supervisor on the machine. A lib test that drove the install fan-out
//! (`recycle_supervisors_all_projects_force`) through that discovery:
//!
//! - checkpointed each live document's CRDT, so the test binary connected to
//!   the operator's editor sockets. It carried no `AGENT_DOC_BUILD_ID`, so its
//!   bare `0.35.509` hello was rejected as an IPC build mismatch;
//! - appended an `install_fanout` recycle request to each live project's
//!   `state.db`, so every open supervisor ran an install recycle with nothing
//!   installed.
//!
//! On 2026-10-15 roughly ten concurrent `make check` runs did this at 19:40,
//! 19:56 and 20:27Z. The bare-version rejections and the recycles were logged
//! in the `haiven-dev` and `agent-loop` ops logs.
//!
//! The scope is the set of directories a test process may treat as runtime
//! state. In a cargo test binary ([`running_as_cargo_test_binary`]) it is
//! [`std::env::temp_dir`]. [`TEST_RUNTIME_SCOPE_ENV`] widens or replaces it
//! explicitly. `make test` and the nextest setup script export it so the
//! `agent-doc` binaries an integration test spawns are confined too. With
//! neither, which is every production process, there is no scope:
//! [`path_in_test_runtime_scope`] is always true and
//! [`guard_test_runtime_target`] never fires.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::root_ceiling::{parse_root_ceilings, running_as_cargo_test_binary};

/// Explicit test runtime scope: a `PATH`-style list of absolute directories.
/// When set to a non-empty list, the scope applies in any process, including a
/// spawned `agent-doc` binary. Empty or unset, only cargo test binaries are
/// scoped (to the temp dir).
pub const TEST_RUNTIME_SCOPE_ENV: &str = "AGENT_DOC_TEST_RUNTIME_SCOPE";

/// The directories this process may treat as runtime state, or `None` when the
/// process is unscoped (production).
pub fn test_runtime_scope() -> Option<Vec<PathBuf>> {
    scope_from(
        std::env::var_os(TEST_RUNTIME_SCOPE_ENV),
        running_as_cargo_test_binary(),
        std::env::temp_dir(),
    )
}

fn scope_from(
    explicit: Option<OsString>,
    test_binary: bool,
    temp_dir: PathBuf,
) -> Option<Vec<PathBuf>> {
    let explicit = explicit
        .map(|value| parse_root_ceilings(&value))
        .filter(|dirs| !dirs.is_empty());
    match (explicit, test_binary) {
        (Some(dirs), _) => Some(dirs),
        (None, true) => Some(parse_root_ceilings(&temp_dir.into_os_string())),
        (None, false) => None,
    }
}

/// Whether `path` lies inside `scope`. A relative path is anchored to the
/// working directory first. Both the written and the canonical spelling are
/// tried, so `/tmp` vs `/private/tmp` and paths that no longer exist both work.
pub fn path_in_scope(path: &Path, scope: &[PathBuf]) -> bool {
    let anchored = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            Err(_) => return false,
        }
    };
    let canonical = canonical_prefix(&anchored);
    scope
        .iter()
        .any(|dir| anchored.starts_with(dir) || canonical.starts_with(dir))
}

/// Canonicalize the longest existing ancestor of `path` and re-append the rest,
/// so a document or root that was already removed still resolves through
/// symlinked parents.
fn canonical_prefix(path: &Path) -> PathBuf {
    let mut suffix = Vec::new();
    let mut probe = path;
    loop {
        if let Ok(canonical) = probe.canonicalize() {
            let mut out = canonical;
            for part in suffix.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (probe.parent(), probe.file_name()) {
            (Some(parent), Some(name)) => {
                suffix.push(name.to_os_string());
                probe = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Whether this process may observe or drive the runtime state at `path`.
/// Always true in production.
pub fn path_in_test_runtime_scope(path: &Path) -> bool {
    test_runtime_scope().is_none_or(|scope| path_in_scope(path, &scope))
}

/// Fail a test that is about to drive runtime state outside its scope.
///
/// `what` names the effect (for example `install recycle request`). This is a
/// panic on purpose: a test that reaches a live project root has already
/// escaped its fixture. Returning an error would let a fail-open caller
/// swallow it, and the leak would go unnoticed again. Production processes
/// have no scope and never reach the panic.
pub fn guard_test_runtime_target(path: &Path, what: &str) {
    let Some(scope) = test_runtime_scope() else {
        return;
    };
    if path_in_scope(path, &scope) {
        return;
    }
    panic!(
        "test process attempted {what} against {} outside its runtime scope [{}]; a test \
         must only drive runtimes under its own temp root, never a live project's sockets, \
         state.db, or install-recycle trigger (#testisolationleaks)",
        path.display(),
        scope
            .iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(":"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_processes_have_no_scope() {
        assert_eq!(scope_from(None, false, PathBuf::from("/tmp")), None);
        assert_eq!(
            scope_from(Some(OsString::new()), false, PathBuf::from("/tmp")),
            None,
            "an empty explicit list does not scope a production process"
        );
    }

    #[test]
    fn a_test_binary_is_scoped_to_its_temp_dir() {
        let scope = scope_from(None, true, PathBuf::from("/nonexistent-tmp-root")).unwrap();
        assert_eq!(scope, vec![PathBuf::from("/nonexistent-tmp-root")]);
    }

    #[test]
    fn an_explicit_scope_applies_to_any_process_and_replaces_the_temp_dir() {
        let value = std::env::join_paths(["/scope/a", "relative", "/scope/b"]).unwrap();
        let expected = vec![PathBuf::from("/scope/a"), PathBuf::from("/scope/b")];
        assert_eq!(
            scope_from(Some(value.clone()), false, PathBuf::from("/tmp")),
            Some(expected.clone())
        );
        assert_eq!(
            scope_from(Some(value), true, PathBuf::from("/tmp")),
            Some(expected)
        );
    }

    #[test]
    fn scope_membership_is_by_path_prefix_not_string_prefix() {
        let scope = vec![PathBuf::from("/scope/tmp")];
        assert!(path_in_scope(Path::new("/scope/tmp"), &scope));
        assert!(path_in_scope(Path::new("/scope/tmp/x/plan.md"), &scope));
        assert!(!path_in_scope(Path::new("/scope/tmpfoo/plan.md"), &scope));
        assert!(!path_in_scope(Path::new("/home/me/work/app"), &scope));
    }

    #[test]
    fn scope_membership_sees_through_a_symlinked_parent_of_a_removed_path() {
        let real = tempfile::tempdir().unwrap();
        let links = tempfile::tempdir().unwrap();
        let link = links.path().join("link");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();
        let scope = vec![real.path().canonicalize().unwrap()];
        assert!(path_in_scope(&link.join("gone/plan.md"), &scope));
    }

    #[test]
    fn this_test_binary_is_scoped_and_its_tempdirs_are_in_scope() {
        let scope = test_runtime_scope().expect("a cargo test binary is always scoped");
        assert!(!scope.is_empty());
        let dir = tempfile::tempdir().unwrap();
        assert!(path_in_test_runtime_scope(dir.path()));
        guard_test_runtime_target(dir.path(), "a tempdir probe");
    }

    #[test]
    fn the_guard_fails_a_test_that_targets_a_live_root() {
        let live = Path::new("/home/operator/work/live-project");
        assert!(!path_in_test_runtime_scope(live));
        let refused = std::panic::catch_unwind(|| {
            guard_test_runtime_target(live, "install recycle request");
        })
        .expect_err("a test reached a live root without failing");
        let message = refused
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(message.contains("#testisolationleaks"), "{message}");
        assert!(message.contains("install recycle request"), "{message}");
    }
}

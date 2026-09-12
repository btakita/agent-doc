//! `#installstrandsreplica` — native cdylib reload admission and the deferred
//! intent that re-arms it.
//!
//! `#deploy-just-do-it` mandates a `make install` after every fix, and install
//! fans a typed `reload_library` intent out to every hot-reload-capable editor
//! process. Retiring a native generation discards the Lazily replicas it owns;
//! the JetBrains handoff re-registers them, but that re-registration does not
//! converge for a document that is attached and mid-cycle. The binary then sees
//! `editor_attached_model_missing`, `missing_replica` recovery exhausts, and the
//! disk descent is correctly refused — so the session that ordered the install
//! cannot close itself out.
//!
//! The gate is [`agent_doc_supervisor::lifecycle::native_reload_admission`], the
//! same open-cycle fact the recycle and restart paths read. A deferred reload is
//! recorded here and published by the owning supervisor's idle watch once the
//! cycle closes, so the editor reaches the current generation without an
//! operator step.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Marker file, under a project's `.agent-doc/`, naming the cdylib version whose
/// reload was deferred because an attached document was mid-cycle.
pub const PENDING_NATIVE_RELOAD_FILE: &str = "pending-lib-reload";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Path of the deferred-reload marker for `project_root`.
pub fn pending_native_reload_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".agent-doc")
        .join(PENDING_NATIVE_RELOAD_FILE)
}

/// Whether `file` holds an open agent-doc cycle that a native generation handoff
/// would strand.
///
/// A *stalled* open cycle does not block: an abandoned older turn must not freeze
/// every editor in the project on the build it happened to load, and the
/// supervisor recycle path already force-closes that shape past the same
/// deadline. An unreadable projection blocks — the safe outcome is the editor
/// keeping a generation that demonstrably works.
pub fn document_cycle_blocks_native_reload(file: &Path) -> bool {
    match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
        Ok(Some(state)) => {
            state.is_open()
                && !state.open_stalled(
                    0,
                    now_secs(),
                    agent_doc_cycle_state_io::STALLED_CYCLE_RESOLVE_SECS,
                )
        }
        Ok(None) => false,
        Err(_) => true,
    }
}

/// Documents a native generation handoff on `project_root` could strand.
///
/// `#editorendpointzero-reloadgate`: the first cut of this gate read the attached
/// set from `reliable_sync_status.registrations` alone, and that record **can be
/// empty while the editor is alive and listening on its PID-scoped socket** — the
/// same hole the fan-out's own socket-discovery fallback exists for. With no
/// candidates there is nothing to find an open cycle on, so the gate published
/// silently. Measured 2026-09-12 18:25 on this project: `agent-doc-bugs.md` held
/// open `cycle-1789236397866` in `state.db`, its replica was registered in the ops
/// log, and `lib-install` still reported `0 deferred mid-cycle`.
///
/// `open_supervisor_documents` is the independent source: it walks live
/// `agent-doc start --route-owned` processes, so it does not consult the editor
/// record at all, and a document with an open cycle is exactly a document with a
/// live supervisor. Both sources are unioned and scoped to this project.
///
/// The result is deliberately **not** narrowed per editor pid. One cdylib serves a
/// whole editor process, so a reload delivered through any endpoint retires the
/// generation holding every replica in that process; per-pid precision would only
/// reintroduce a blind spot. An over-broad defer costs an editor the previous build
/// until the idle boundary, which is the cheap direction.
pub fn native_reload_candidate_documents<R, S>(
    registration_paths: R,
    supervisor_documents: S,
    project_root: &Path,
) -> Vec<PathBuf>
where
    R: IntoIterator<Item = String>,
    S: IntoIterator<Item = PathBuf>,
{
    let mut candidates: BTreeSet<PathBuf> = BTreeSet::new();
    for path in registration_paths {
        if path.is_empty() {
            continue;
        }
        candidates.insert(PathBuf::from(path));
    }
    candidates.extend(supervisor_documents);
    candidates
        .into_iter()
        .filter(|path| path.starts_with(project_root))
        .collect()
}

/// Record that `lib_version`'s reload was deferred for `project_root`.
///
/// Best-effort: the marker only re-arms a fan-out that install already reported,
/// so a project with no writable `.agent-doc/` loses the re-arm, never the
/// deferral that protects the open cycle.
pub fn record_pending_native_reload(project_root: &Path, lib_version: &str) {
    let path = pending_native_reload_path(project_root);
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let _ = std::fs::write(&path, lib_version);
}

/// The cdylib version whose reload is pending for `project_root`, if any.
pub fn read_pending_native_reload(project_root: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(pending_native_reload_path(project_root)).ok()?;
    let version = raw.trim();
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

/// Drop the deferred-reload marker for `project_root`.
pub fn clear_pending_native_reload(project_root: &Path) {
    let _ = std::fs::remove_file(pending_native_reload_path(project_root));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).expect("state dir");
        dir
    }

    /// `#installstrandsreplica`: the fact the gate reads, measured against real
    /// cycle state on both sides of closeout. An open cycle is exactly the
    /// window in which retiring the native generation strands the document's
    /// replica; once the cycle commits, holding the editor back would freeze it
    /// on a stale build for nothing.
    #[test]
    fn open_cycle_blocks_the_reload_and_a_committed_one_releases_it() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body").expect("write doc");

        agent_doc_cycle_state_io::start_preflight(&doc, Some("snap"), Some("body"))
            .expect("open cycle");
        assert!(
            document_cycle_blocks_native_reload(&doc),
            "an attached document mid-cycle must defer the native generation handoff"
        );

        agent_doc_cycle_state_io::mark_committed(&doc, "evt", Some("snap"), Some("body"))
            .expect("commit cycle");
        assert!(
            !document_cycle_blocks_native_reload(&doc),
            "a committed cycle must publish the reload instead of pinning the old generation"
        );
    }

    /// `#editorendpointzero-reloadgate`: the registration record can be empty while
    /// the editor is alive, and reading the attached set from it alone published the
    /// reload with nothing to check. The supervisor walk does not consult that record.
    #[test]
    fn an_empty_registration_record_still_yields_the_live_supervisor_documents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let doc = root.join("tasks/agent-doc/agent-doc-bugs.md");

        let candidates =
            native_reload_candidate_documents(Vec::new(), vec![doc.clone()], root);
        assert_eq!(
            candidates,
            vec![doc],
            "a live supervisor's document must reach the gate even with no editor registration"
        );
    }

    /// The two sources overlap in the healthy case and must not double-count, and a
    /// document served by a different project is not this fan-out's to defer on.
    #[test]
    fn candidates_are_deduplicated_and_scoped_to_the_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let doc = root.join("plan.md");
        let foreign = PathBuf::from("/elsewhere/other.md");

        let candidates = native_reload_candidate_documents(
            vec![
                doc.to_string_lossy().to_string(),
                String::new(),
                foreign.to_string_lossy().to_string(),
            ],
            vec![doc.clone(), foreign],
            root,
        );
        assert_eq!(candidates, vec![doc]);
    }

    #[test]
    fn pending_marker_round_trips_and_clears() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        assert_eq!(read_pending_native_reload(root), None);
        record_pending_native_reload(root, "0.35.370");
        assert_eq!(
            read_pending_native_reload(root),
            Some("0.35.370".to_string()),
        );
        clear_pending_native_reload(root);
        assert_eq!(read_pending_native_reload(root), None);
    }

    /// A blank marker is not a version. Treating it as one would fan a
    /// `reload_library` intent out announcing an empty `lib_version`.
    #[test]
    fn blank_marker_reads_as_no_pending_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).expect("state dir");
        std::fs::write(pending_native_reload_path(root), "  \n").expect("write marker");
        assert_eq!(read_pending_native_reload(root), None);
    }

    /// A document with no cycle state at all is not mid-cycle, so an ordinary
    /// install into a quiet project must still publish immediately.
    #[test]
    fn document_without_cycle_state_does_not_block_the_reload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "# plan\n").expect("write doc");
        assert!(!document_cycle_blocks_native_reload(&file));
    }
}

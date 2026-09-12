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

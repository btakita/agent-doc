//! Live wiring of the CRDT relay/commit-barrier into the finalize + disk paths
//! (`#crdtauth4` cutover).
//!
//! The state-vector sync primitive ([`agent_doc_core::crdt_sync`]), the authority
//! state machine ([`crate::crdt_authority`]), and the relay hub
//! ([`crate::crdt_relay`]) were built and tested as standalone modules. This
//! module is the **live cutover**: it routes the real `finalize` commit point and
//! the real `.yrs` load/merge call-sites through the authority-gated barrier,
//! while keeping the headless / [`CrdtAuthority::GitAuthoritative`] path
//! byte-for-byte unchanged.
//!
//! ## Authority gate is load-bearing
//!
//! Every entry point here resolves the document's [`CrdtAuthority`] first (cheaply,
//! per-document, fail-safe to `GitAuthoritative`) via
//! [`crate::crdt_authority::authority_for_file`]:
//!
//! - [`CrdtAuthority::GitAuthoritative`] (**Detached** — no live editor): every
//!   entry point is a **no-op** that returns the trivially-ready / unchanged
//!   result. The CRDT is ephemeral, git is the source of truth, and none of the
//!   live-replica machinery runs. This is most dogfooding traffic and it is
//!   provably unchanged (see the tests at the bottom).
//! - [`CrdtAuthority::MultiReplica`] (**EditorAttached** — a live editor plugin):
//!   the commit barrier flushes the currently-live editor replicas to a consistent
//!   cut before the snapshot is committed, and the disk `.yrs` is treated as a
//!   write-through recovery projection only (in-memory wins).
//!
//! ## Per-document isolation (`#xdocsuper1/3`)
//!
//! The hub registry is keyed by the document hash
//! ([`crate::snapshot::doc_hash`]). Each document gets its own independent
//! [`RelayHub`]; a hub for one document can never observe or flush another
//! document's replicas. This is the same per-document isolation the hosting-epoch
//! backbone enforces, applied to the live relay layer.
//!
//! ## Scope of this cutover
//!
//! - **Wired:** the finalize commit barrier ([`commit_barrier_for_file`]), the
//!   disk-demotion reconcile at the live load seam
//!   ([`reconcile_disk_projection_for_file`]), supervisor-restart recovery of the
//!   canonical replica from the disk projection ([`recover_hub_from_disk`]), and
//!   the per-document hub registry ([`with_hub`]).
//! - **Deferred (reported, not forced):** fan-out of editor yrs deltas over a
//!   *live socket*. Today editor writes reach the supervisor as markdown patch
//!   files (`.agent-doc/patches/<hash>.json`) + ack sidecars, not as yrs updates,
//!   and the supervisor IPC socket carries a fixed control-message enum
//!   ([`crate::supervisor::ipc::IpcMethod`]), not document deltas. A live
//!   delta-fan-out path needs a new IPC message family plus plugin-side yrs-delta
//!   forwarding — a structural change beyond this additive, authority-gated
//!   cutover. The relay hub is wired so that *when* deltas arrive (via FFI replica
//!   ABI or a future delta message) they flow through [`with_hub`]; the barrier
//!   already flushes whatever replicas are registered.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::Result;

use crate::crdt_authority::{CrdtAuthority, authority_for_file};
use crate::crdt_relay::RelayHub;

/// The canonical replica's reserved yrs client-id for every per-document hub. The
/// supervisor's canonical replica is the hub authority; editor replicas mint
/// their own ids via [`crate::crdt_relay::mint_client_id`] and can never collide
/// with this reserved id (`RelayHub::register` rejects it).
const CANONICAL_CLIENT_ID: u64 = 1;

/// Process-global per-document relay-hub registry, keyed by document hash.
///
/// Per-document isolation (`#xdocsuper1/3`): each document's replicas live in
/// their own hub; there is no shared canonical replica across documents.
fn hub_registry() -> &'static Mutex<HashMap<String, RelayHub>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, RelayHub>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run `f` against the per-document [`RelayHub`] for `file`, creating an empty hub
/// on first contact. This is the single entry point for the live relay layer:
/// register/deregister editor replicas, deliver deltas, and drive the commit
/// barrier all go through here so per-document isolation is structural.
///
/// Returns the closure's result. Does NOT consult authority — callers that must
/// gate on `EditorAttached` should resolve [`authority_for_file`] first (the
/// finalize/disk entry points below do).
pub fn with_hub<T>(file: &Path, f: impl FnOnce(&mut RelayHub) -> T) -> Result<T> {
    let hash = crate::snapshot::doc_hash(file)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    let hub = registry
        .entry(hash)
        .or_insert_with(|| RelayHub::new(CANONICAL_CLIENT_ID));
    Ok(f(hub))
}

/// Recover the per-document canonical replica from a durable disk recovery
/// projection on supervisor restart (plan phase 6). At most one flush is lost;
/// live editors re-sync newer ops when they re-register. The disk `.yrs` is a
/// recovery input only, never authority.
///
/// Idempotent on an existing hub: if a live hub for the document already exists,
/// the stale disk projection is reconciled into it (in-memory wins) rather than
/// replacing it.
pub fn recover_hub_from_disk(file: &Path, projection: &[u8]) -> Result<()> {
    let hash = crate::snapshot::doc_hash(file)?;
    let mut registry = hub_registry()
        .lock()
        .map_err(|e| anyhow::anyhow!("relay hub registry poisoned: {e}"))?;
    match registry.get(&hash) {
        // A live hub already holds the authority — disk is recovery-only, so
        // reconcile the projection into it (in-memory wins) instead of clobbering.
        Some(existing) => {
            existing.reconcile_disk_projection(projection)?;
            Ok(())
        }
        None => {
            let hub = RelayHub::recover_from_projection(CANONICAL_CLIENT_ID, projection)?;
            registry.insert(hash, hub);
            Ok(())
        }
    }
}

/// The **authority-gated commit barrier** at the live finalize commit point
/// (`#crdtauth4`, plan phase 4).
///
/// This replaces the fragile patch-ack quiescence proof for the EditorAttached
/// path: before the snapshot is committed to git, every currently-live editor
/// replica is flushed into the canonical replica on a **consistent cut**, so a
/// commit can only snapshot a state that provably holds every live editor's last
/// ops. It is a checkpoint, **not a global lock** — a slow / disconnected editor
/// is excluded from the cut (and contributes on reconnect), so finalize never
/// blocks forever on a stalled editor.
///
/// Returns whether a snapshot is safe to commit:
/// - [`CrdtAuthority::GitAuthoritative`] (**Detached**): trivially `true`, no hub
///   work, no allocation of a hub — the headless commit path is unchanged.
/// - [`CrdtAuthority::MultiReplica`] (**EditorAttached**): drives the per-document
///   hub's commit barrier and returns its consistent-cut result.
///
/// Failures are logged and treated as a non-fatal "proceed" so the barrier can
/// never wedge a live closeout (the existing patch-ack / CRDT-merge path remains
/// the durable fallback). The authority resolution itself is the only thing that
/// gates behavior.
pub fn commit_barrier_for_file(file: &Path) -> bool {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    commit_barrier_for_file_with_authority(file, authority)
}

/// [`commit_barrier_for_file`] with an explicitly-resolved authority — the
/// deterministically-testable core. Callers that already hold a resolved
/// [`CrdtAuthority`] (e.g. from a backbone projection) should use this to avoid a
/// second lease read.
pub fn commit_barrier_for_file_with_authority(file: &Path, authority: CrdtAuthority) -> bool {
    if !authority.editor_attached() {
        // Detached / headless: the CRDT is ephemeral, git is the source of truth,
        // and there are no live editor replicas to flush. The barrier is trivially
        // satisfied and NO hub is touched — the headless path is byte-for-byte
        // unchanged.
        return true;
    }
    match with_hub(file, |hub| hub.commit_barrier_under_authority(authority)) {
        Ok(Ok(ready)) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_commit_barrier file={} authority=multi_replica ready={} live_editors={}",
                    file.display(),
                    ready,
                    with_hub(file, |hub| hub.live_count()).unwrap_or(0),
                ),
            );
            ready
        }
        Ok(Err(e)) => {
            // A barrier error must never wedge a live closeout — log and proceed
            // on the existing patch-ack / CRDT-merge fallback.
            crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_commit_barrier_error file={} authority=multi_replica error={}",
                    file.display(),
                    e
                ),
            );
            true
        }
        Err(e) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "crdt_commit_barrier_registry_error file={} error={}",
                    file.display(),
                    e
                ),
            );
            true
        }
    }
}

/// The **authority-gated disk-demotion reconcile** at the live `.yrs` load seam
/// (`#crdtauth4`, plan phase 6).
///
/// Under [`CrdtAuthority::MultiReplica`] the in-memory canonical replica is the
/// authority and the disk `.yrs` is a write-through **recovery projection only**
/// ([`crate::crdt_relay::DISK_IS_RECOVERY_PROJECTION_ONLY`]): a (possibly stale)
/// disk projection is reconciled INTO the live replica, which can only add ops the
/// live replica genuinely lost (a crash gap) and can never regress live text —
/// in-memory wins. Returns `Some(changed)` where `changed` is whether the disk
/// held ops the live replica was missing.
///
/// Under [`CrdtAuthority::GitAuthoritative`] there is no live in-memory authority
/// to reconcile against — disk demotion does not apply, and the existing
/// baseline-wins load path ([`crate::snapshot::crdt_merge_base_state`], which
/// already discards a stale `.yrs` whose markdown projection does not match the
/// cycle baseline) is left to run unchanged. Returns `None` (no live reconcile
/// performed).
pub fn reconcile_disk_projection_for_file(file: &Path, projection: &[u8]) -> Result<Option<bool>> {
    let file_str = file.display().to_string();
    let authority = authority_for_file(&file_str);
    if !authority.editor_attached() {
        // Headless: no live canonical replica is authoritative. The
        // baseline-wins load path in snapshot.rs already handles stale disk.
        return Ok(None);
    }
    let changed = with_hub(file, |hub| hub.reconcile_disk_projection(projection))??;
    crate::ops_log::log_op(
        file,
        &format!(
            "crdt_disk_demotion_reconcile file={} authority=multi_replica disk_added_ops={}",
            file.display(),
            changed
        ),
    );
    Ok(Some(changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crdt_relay::mint_client_id;
    use std::io::Write;

    /// A throwaway tracked document under a temp project root so `doc_hash` and the
    /// per-document keying resolve against a real path.
    fn temp_doc(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        // `.agent-doc/` marks the project root for `find_project_root`.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "# {name}\n\nbody").unwrap();
        (dir, path)
    }

    #[test]
    fn detached_commit_barrier_is_a_trivial_noop() {
        // Detached / GitAuthoritative: the barrier is trivially ready and NO hub is
        // allocated for the document — the headless commit path is untouched.
        let (_dir, doc) = temp_doc("detached.md");
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::GitAuthoritative
        ));
        let registry = hub_registry().lock().unwrap();
        assert!(
            !registry.contains_key(&hash),
            "the Detached path must not allocate a relay hub"
        );
    }

    #[test]
    fn editor_attached_commit_barrier_flushes_live_replicas_on_a_consistent_cut() {
        // EditorAttached / MultiReplica: a live editor replica with an un-flushed
        // local op is flushed into the canonical replica at the barrier, and the
        // committed cut holds the editor's keystrokes.
        let (_dir, doc) = temp_doc("attached.md");
        let editor = mint_client_id("intellij:attached-test");
        with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            // Editor types locally; the op is NOT yet relayed to canonical.
            hub.local_edit(editor, 0, 0, "typed-before-commit").unwrap();
            assert!(
                !hub.canonical_text().contains("typed-before-commit"),
                "the un-relayed op is not in canonical before the barrier"
            );
        })
        .unwrap();

        // The barrier flushes the live editor into canonical (consistent cut).
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&doc, |hub| {
            assert!(
                hub.canonical_text().contains("typed-before-commit"),
                "the barrier flushed the live editor's op into the committed cut"
            );
        })
        .unwrap();
    }

    #[test]
    fn editor_attached_commit_barrier_does_not_block_on_a_disconnected_editor() {
        // A slow / disconnected editor must NOT deadlock the commit barrier — its
        // op is excluded from the live cut and contributes on reconnect.
        let (_dir, doc) = temp_doc("disconnected.md");
        let live = mint_client_id("vscode:live");
        let slow = mint_client_id("intellij:slow");
        with_hub(&doc, |hub| {
            hub.register(live).unwrap();
            hub.register(slow).unwrap();
            hub.local_edit(live, 0, 0, "LIVE").unwrap();
            hub.local_edit(slow, 0, 0, "SLOW").unwrap();
            // The slow editor disconnects with an un-flushed op.
            hub.disconnect(slow);
        })
        .unwrap();

        // The barrier returns ready WITHOUT blocking; the live op is in the cut,
        // the disconnected op is not.
        assert!(commit_barrier_for_file_with_authority(
            &doc,
            CrdtAuthority::MultiReplica
        ));
        with_hub(&doc, |hub| {
            let cut = hub.canonical_text();
            assert!(cut.contains("LIVE"), "the live editor's op is in the cut");
            assert!(
                !cut.contains("SLOW"),
                "the disconnected editor's op is excluded (no deadlock)"
            );
            // No data loss: the slow editor contributes on reconnect.
            hub.reconnect(slow).unwrap();
            assert!(hub.canonical_text().contains("SLOW"));
        })
        .unwrap();
    }

    #[test]
    fn disk_demotion_in_memory_wins_at_the_live_load_seam() {
        // EditorAttached: a STALE disk projection reconciled at the live load seam
        // must not regress the live in-memory text (in-memory wins).
        let (_dir, doc) = temp_doc("demotion.md");
        let editor = mint_client_id("intellij:demotion");
        let stale_projection = with_hub(&doc, |hub| {
            hub.register(editor).unwrap();
            hub.apply_local(editor, 0, 0, "v1").unwrap();
            // Flush a durable recovery projection (what hits .yrs) at "v1".
            let proj = hub.projection_bytes();
            // The live session advances past the projection to "v1 v2".
            let len = hub.canonical_text().chars().count() as u32;
            hub.apply_local(editor, len, 0, " v2").unwrap();
            assert_eq!(hub.canonical_text(), "v1 v2");
            proj
        })
        .unwrap();

        // Reconciling the STALE disk projection holds no new ops and never regresses.
        let changed = reconcile_disk_projection_for_file_with_authority(
            &doc,
            &stale_projection,
            CrdtAuthority::MultiReplica,
        )
        .unwrap();
        assert_eq!(changed, Some(false), "a stale disk projection adds no ops");
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), "v1 v2", "in-memory replica wins");
        })
        .unwrap();
    }

    #[test]
    fn disk_demotion_is_skipped_on_the_headless_path() {
        // GitAuthoritative: no live in-memory authority — the live reconcile is
        // skipped (the baseline-wins snapshot load path runs unchanged) and no hub
        // is allocated.
        let (_dir, doc) = temp_doc("headless-demotion.md");
        let hash = crate::snapshot::doc_hash(&doc).unwrap();
        let result = reconcile_disk_projection_for_file_with_authority(
            &doc,
            b"any-bytes-are-ignored",
            CrdtAuthority::GitAuthoritative,
        )
        .unwrap();
        assert_eq!(result, None, "the headless path performs no live reconcile");
        assert!(
            !hub_registry().lock().unwrap().contains_key(&hash),
            "the headless path must not allocate a relay hub"
        );
    }

    #[test]
    fn recover_hub_from_disk_rebuilds_canonical_on_restart() {
        // Supervisor restart: rebuild the canonical replica from the last disk
        // recovery projection; members re-register / re-sync afterward.
        let (_dir, doc) = temp_doc("recover.md");
        // Build a projection from a throwaway hub (simulating a prior session).
        let mut prior = RelayHub::new(CANONICAL_CLIENT_ID);
        let ed = mint_client_id("intellij:prior");
        prior.register(ed).unwrap();
        prior.apply_local(ed, 0, 0, "durable").unwrap();
        let projection = prior.projection_bytes();

        recover_hub_from_disk(&doc, &projection).unwrap();
        with_hub(&doc, |hub| {
            assert_eq!(hub.canonical_text(), "durable");
            assert_eq!(hub.live_count(), 0, "members re-register after restart");
        })
        .unwrap();
    }

    /// Test-only authority-explicit variant of [`reconcile_disk_projection_for_file`]
    /// so the demotion seam is deterministically exercisable without a live lease.
    fn reconcile_disk_projection_for_file_with_authority(
        file: &Path,
        projection: &[u8],
        authority: CrdtAuthority,
    ) -> Result<Option<bool>> {
        if !authority.editor_attached() {
            return Ok(None);
        }
        let changed = with_hub(file, |hub| hub.reconcile_disk_projection(projection))??;
        Ok(Some(changed))
    }
}

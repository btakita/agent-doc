//! # Module: snapshot
//!
//! ## Spec
//! - `doc_hash(doc)`: compute SHA-256 hex of the document's canonical absolute path.
//!   Used as a stable, collision-resistant filename key for all per-doc state files.
//! - `find_project_root(path)`: walk up directory tree to find the directory containing
//!   `.agent-doc/`. Returns `None` if not found (e.g., in tests without project scaffolding).
//! - `path_for(doc)`: compute snapshot path `<project_root>/.agent-doc/snapshots/<hash>.md`.
//!   Falls back to a relative path when no project root is found.
//! - `lock_path_for(doc)`: compute advisory lock path `<project_root>/.agent-doc/locks/<hash>.lock`.
//! - `pending_path_for(doc)`: compute pending response path `<project_root>/.agent-doc/pending/<hash>.md`.
//! - `crdt_path_for(doc)`: compute legacy text CRDT state path
//!   `<project_root>/.agent-doc/crdt/<hash>.yrs`.
//! - `overlay_crdt_path_for(doc)`: compute structured markdown-overlay CRDT
//!   state path `<project_root>/.agent-doc/crdt/<hash>.overlay.yrs`.
//! - `pre_response_path_for(doc)`: compute pre-response snapshot path
//!   `<project_root>/.agent-doc/pre-response/<hash>.md`.
//! - `SnapshotLock::acquire(doc)`: acquire an exclusive advisory flock on the snapshot lock
//!   file. Blocks until available. Released on drop.
//! - `load(doc)`: acquire snapshot lock, return snapshot content or `None` if absent.
//! - `save(doc, content)`: acquire snapshot lock, atomically write content via tempfile+rename.
//! - `delete(doc)`: acquire snapshot lock, remove snapshot file if present. No-op if absent.
//! - `resolve(doc)`: authoritative baseline for merge. Returns snapshot file content when it
//!   exists. Falls back to `git show HEAD:<doc>` only when no snapshot file exists and git
//!   content differs from current file (recovery path). Returns `None` on first submit.
//! - `save_pre_response(doc, content)`: atomically write content to the pre-response path
//!   (saved before the agent's response is applied, enabling undo).
//! - `load_pre_response(doc)`: return pre-response snapshot content or `None`.
//! - `delete_pre_response(doc)`: remove pre-response snapshot if present.
//! - `load_crdt(doc)`: acquire CRDT advisory lock, return CRDT state bytes or `None`.
//! - `save_crdt(doc, state)`: acquire CRDT advisory lock, atomically write legacy text state bytes.
//! - `crdt_merge_base_state(doc, fallback_markdown)`: build the merge-base
//!   CRDT state from the structured overlay sidecar when it matches the current
//!   cycle baseline, falling back to `fallback_markdown` otherwise.
//! - `save_document_crdt(doc, state, markdown)`: persist both the legacy text
//!   state and the structured markdown-overlay state derived from the same
//!   runtime markdown snapshot.
//! - `delete_crdt(doc)`: acquire CRDT advisory lock, remove CRDT state file if present.
//!
//! ## Agentic Contracts
//! - All writes (snapshot, pre-response, CRDT) are atomic: written to a temp file in the
//!   same directory then renamed, ensuring no partial reads under concurrent access.
//! - `load`, `save`, `delete`, `load_crdt`, `save_crdt`,
//!   `crdt_merge_base_state`, `save_document_crdt`, and `delete_crdt` are all
//!   flock-protected — safe to call from concurrent processes on the same
//!   document.
//! - `resolve` prefers the snapshot file unconditionally when it exists. Git is only used
//!   as a recovery fallback. This prevents false baselines after a step-0b commit.
//! - `doc_hash` is deterministic and stable: same canonical path → same hash across runs.
//! - `path_for`, `lock_path_for`, `pending_path_for`, `crdt_path_for`,
//!   `overlay_crdt_path_for`, and `pre_response_path_for` all use the same
//!   `doc_hash`, so files for the same document always colocate under the same
//!   project root `.agent-doc/` tree.
//! - `delete` and `delete_crdt` are idempotent: calling them on an absent file is not an error.
//! - Pre-response snapshots are not flock-protected (single-writer assumption: only the
//!   active write path saves them).
//! - `try_migrate_renamed(doc)` auto-detects orphaned state files after a rename by matching
//!   the document's session UUID against snapshots. Migrates all state file types (snapshots,
//!   baselines, locks, pending, crdt, pre-response) and updates the sessions registry.
//!
//! ## Evals
//! - `path_for_consistent_hash`: calling `path_for` twice on the same doc returns equal paths.
//! - `path_for_different_files_different_hashes`: two distinct files → distinct snapshot paths.
//! - `path_for_has_correct_structure`: path contains `.agent-doc/snapshots/`, ends with `.md`,
//!   stem is 64 lowercase hex chars.
//! - `load_returns_none_when_no_snapshot`: no snapshot file → `load` returns `None`.
//! - `snapshot_write_and_read_directly`: write then read snapshot file → content round-trips.
//! - `snapshot_overwrite`: writing twice → second value persists.
//! - `snapshot_delete_by_removing_file`: remove snapshot file → `read` returns `None`.
//! - `delete_no_error_when_missing`: `delete` on absent snapshot → no error.
//! - `flock_acquire_and_release_on_drop`: flock released after drop → second acquire succeeds.
//! - `flock_serializes_concurrent_access`: 10 threads increment a counter under flock →
//!   final value is exactly 10 (no lost updates).
//! - `atomic_write_via_tempfile_produces_correct_content`: tempfile+rename → correct content.
//! - `atomic_write_overwrites_existing`: atomic write over existing file → new content visible.
//! - `crdt_path_has_correct_extension`: CRDT path contains `.agent-doc/crdt/`, ends `.yrs`.
//! - `crdt_save_and_load_roundtrip`: save bytes then load → same bytes returned.
//! - `crdt_load_returns_none_when_missing`: no CRDT file → `load_crdt` returns `None`.
//! - `crdt_delete_removes_file`: save then delete → `load_crdt` returns `None`.
//! - `crdt_delete_no_error_when_missing`: `delete_crdt` on absent file → no error.
//! - `concurrent_atomic_writes_no_partial_content`: 20 threads atomically overwrite same
//!   file → final content is exactly one complete write (no corruption).
//! - `resolve_prefers_snapshot_over_git`: snapshot file present with different content than
//!   disk → `resolve` returns snapshot content, not disk/git content.
//! - `try_migrate_renamed_finds_orphaned_snapshot`: snapshot under old hash with matching
//!   session UUID → migrated to new hash, all state file types moved.
//! - `try_migrate_renamed_noop_when_snapshot_exists`: snapshot already exists for current
//!   hash → returns false, no migration.
//! - `try_migrate_renamed_noop_when_no_session`: document without session UUID → returns
//!   false, no migration.
//! - `try_migrate_renamed_noop_when_no_match`: no snapshot has matching UUID → returns false.

use anyhow::{Context, Result};
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

const SNAP_DIR: &str = ".agent-doc/snapshots";
const BASELINE_DIR: &str = ".agent-doc/baselines";
const LOCK_DIR: &str = ".agent-doc/locks";
const PENDING_DIR: &str = ".agent-doc/pending";
const CRDT_DIR: &str = ".agent-doc/crdt";

/// Compute the SHA256 hex hash of a document's canonical path.
/// Used for both snapshot filenames and lock filenames.
pub fn doc_hash(doc: &Path) -> Result<String> {
    let canonical = doc.canonicalize()?;
    Ok(hash_path_str(&canonical.to_string_lossy()))
}

/// Compute the SHA256 hex hash from an absolute path string.
///
/// Unlike [`doc_hash`], this does not call `canonicalize()` and therefore works
/// for paths that no longer exist on disk (e.g., the old path after a rename).
pub fn doc_hash_from_str(absolute_path: &str) -> String {
    hash_path_str(absolute_path)
}

fn hash_path_str(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute the advisory lock file path for a given document.
/// Walks up from the document to find the `.agent-doc/` project root.
/// Returns `<project_root>/.agent-doc/locks/<sha256_hash>.lock`.
/// Falls back to the document's parent directory if no project root found.
pub fn lock_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(LOCK_DIR).join(format!("{}.lock", hash)))
}

/// Compute the pending response file path for a given document.
/// Returns `<project_root>/.agent-doc/pending/<sha256_hash>.md`.
pub fn pending_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(PENDING_DIR).join(format!("{}.md", hash)))
}

// `find_project_root` moved to `agent_doc_orchestration::fs_util` (Direction A,
// increment 4). Re-exported here so `snapshot::find_project_root` call sites and
// this module's internal unqualified callers resolve unchanged.
pub use crate::fs_util::find_project_root;

// ---------------------------------------------------------------------------
// Advisory file lock for snapshot operations
// ---------------------------------------------------------------------------

/// RAII guard for exclusive advisory lock on a snapshot file.
///
/// Acquire via `SnapshotLock::acquire(doc_path)`. The lock file is
/// `<snapshot_path>.lock` (sibling file). The lock is released when the
/// guard is dropped.
pub struct SnapshotLock {
    _file: File,
    lock_path: PathBuf,
}

impl SnapshotLock {
    /// Acquire an exclusive advisory lock for the snapshot of the given document.
    /// Blocks until the lock is available.
    pub fn acquire(doc: &Path) -> Result<Self> {
        let snap = path_for(doc)?;
        let lock_path = snap.with_extension("md.lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("failed to open snapshot lock {}", lock_path.display()))?;
        file.lock_exclusive().with_context(|| {
            format!("failed to acquire snapshot lock on {}", lock_path.display())
        })?;
        Ok(Self {
            _file: file,
            lock_path,
        })
    }
}

impl Drop for SnapshotLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
        // Delete the lock file on release to prevent stale lock accumulation.
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// Compute the snapshot file path for a given document.
/// Returns an absolute path: `<project_root>/.agent-doc/snapshots/<hash>.md`.
/// Falls back to relative path if no project root found (e.g., tests without `.agent-doc/`).
pub fn path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let filename = format!("{}.md", hash);
    // Try to find project root for absolute path (consistent with lock_path_for/pending_path_for)
    if let Ok(canonical) = doc.canonicalize()
        && let Some(root) = find_project_root(&canonical)
    {
        return Ok(root.join(SNAP_DIR).join(filename));
    }
    // Fallback: relative path (legacy behavior for tests without .agent-doc/)
    Ok(PathBuf::from(SNAP_DIR).join(filename))
}

/// Compute the preflight baseline file path for a given document.
/// Returns `<project_root>/.agent-doc/baselines/<hash>.md`.
pub fn baseline_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(BASELINE_DIR).join(format!("{}.md", hash)))
}

/// Load the snapshot content under an exclusive lock.
pub fn load(doc: &Path) -> Result<Option<String>> {
    let snap = path_for(doc)?;
    if !snap.exists() {
        return Ok(None);
    }
    let _lock = SnapshotLock::acquire(doc)?;
    load_unlocked(doc)
}

/// Save the current document content as the snapshot under an exclusive lock.
pub fn save(doc: &Path, content: &str) -> Result<()> {
    let _lock = SnapshotLock::acquire(doc)?;
    save_unlocked(doc, content)?;
    crate::ops_log::log_op(
        doc,
        &format!("snapshot_save file={} len={}", doc.display(), content.len()),
    );
    probe_overlay_projection(doc, content);
    Ok(())
}

// ---------------------------------------------------------------------------
// `#mps` Rung 1 (shadow) — model-projected snapshot baseline precondition
// ---------------------------------------------------------------------------
//
// The #mps migration (`tasks/agent-doc/plan-model-projected-snapshot.md`) re-homes
// the merge baseline from the on-disk `.md` snapshot onto the structured document
// model, projecting it on demand from a per-cycle pinned overlay version. Its
// load-bearing precondition (obstacle #2) is that `model → markdown` projection is
// **byte-stable** — otherwise capture-replay hash validation and the merge base
// itself drift. Rung 1 ships only the *instrument* that proves this on real
// traffic; it never changes a persisted artifact or a merge outcome.

/// Round-trip `content` through the exact persist→reload→project pipeline the
/// merge base uses (`OverlayCrdtDoc::from_markdown → encode_state → decode_state →
/// to_markdown`, mirroring [`crdt_merge_base_state`]). Returns the projected
/// markdown, or `None` if the overlay failed to decode/project (logged, never
/// swallowed). Pure (no I/O).
fn project_overlay_roundtrip(content: &str) -> Option<String> {
    let state = agent_doc_markdown_ast::crdt::OverlayCrdtDoc::from_markdown(content).encode_state();
    match agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state(&state) {
        Ok(overlay) => match overlay.to_markdown() {
            Ok(markdown) => Some(markdown),
            Err(e) => {
                eprintln!("[mps] overlay projection (to_markdown) failed: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("[mps] overlay projection (decode_state) failed: {e}");
            None
        }
    }
}

/// Whether the structured overlay model can faithfully project `content` back to
/// byte-identical markdown — the #mps byte-stable-projection gate. Pure; safe to
/// call from evals and (future) cutover preconditions.
pub fn overlay_projection_is_byte_stable(content: &str) -> bool {
    project_overlay_roundtrip(content).as_deref() == Some(content)
}

/// `#mps` Rung 1 shadow probe, wired into [`save`]. **Off by default**; set
/// `AGENT_DOC_MPS_PROJECTION_PROBE=1` during dog-fooding to emit a grep-able
/// `mps_projection_equiv ok|drift …` ops.log marker on every snapshot save,
/// collecting real-traffic byte-stability evidence without imposing the overlay
/// encode/decode cost on every user's hot path. Never fails the cycle.
fn probe_overlay_projection(doc: &Path, content: &str) {
    if std::env::var_os("AGENT_DOC_MPS_PROJECTION_PROBE").is_none() {
        return;
    }
    match project_overlay_roundtrip(content) {
        Some(projected) if projected == content => {
            crate::ops_log::log_op(
                doc,
                &format!(
                    "mps_projection_equiv ok len={} file={}",
                    content.len(),
                    doc.display()
                ),
            );
        }
        Some(projected) => {
            crate::ops_log::log_op(
                doc,
                &format!(
                    "mps_projection_equiv drift kind=mismatch len={} projected_len={} file={}",
                    content.len(),
                    projected.len(),
                    doc.display()
                ),
            );
        }
        None => {
            crate::ops_log::log_op(
                doc,
                &format!(
                    "mps_projection_equiv drift kind=decode_error len={} file={}",
                    content.len(),
                    doc.display()
                ),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// `#mps` Rungs 2-4 — model-projected baseline (opt-in cutover)
// ---------------------------------------------------------------------------
//
// Rung 1 (above) proved `model → markdown` projection is byte-stable. Rungs 2-4
// re-home the *merge baseline* (the explicit `--baseline-file` the finalize merge
// uses as its common ancestor) from the on-disk `.md` baseline onto the structured
// document model, projecting it on demand:
//
//   - Rung 2 (pin): preflight persists the baseline as an overlay sidecar
//     (`baselines/<hash>.overlay.yrs`) representing the exact `resolve_current_doc`
//     projection it already writes to the `.md` baseline.
//   - Rung 3 (flip): the finalize merge sources its base by projecting that overlay
//     instead of reading the `.md`, cross-checking against the `.md` and logging
//     any divergence.
//   - Rung 4 (derive): the `.md` baseline becomes a derived cross-check cache, no
//     longer the read authority. (The file is retained as the cache until live
//     divergence logs are clean; full removal is a later, separate step.)
//
// The whole cutover is gated behind `AGENT_DOC_MPS=1` so it can be exercised
// end-to-end in a dog-food session and reverted instantly (flag off == current
// behavior, byte for byte). Every decision emits a grep-able ops.log marker
// (`mps_baseline_pin`, `mps_baseline_resolve`, `mps_baseline_divergence`).
//
// The baseline overlay is deliberately a *separate* sidecar from the crdt-runtime
// overlay (`overlay_crdt_path_for`) so re-homing the baseline never perturbs the
// stream/crdt write-merge base. The merge baseline is *not* secret-redacted, to
// match the existing un-redacted `.md` baseline (`save_baseline_content`) so the
// cross-check stays byte-exact; the redacted snapshot path is unaffected.

const BASELINE_OVERLAY_EXT: &str = "overlay.yrs";

/// Whether the `#mps` model-projected-baseline cutover (rungs 2-4) is enabled.
///
/// **Default ON.** Opt out with `AGENT_DOC_MPS` set to `0` / `false` / `no` /
/// `off` (case-insensitive) to revert to the pure `.md`-baseline path. Defaulting
/// on is safe because the cutover is non-regressing by construction while the
/// `.md` baseline remains the cross-check cache (Rung 4 not yet complete): the
/// model projection is used only when it agrees with the `.md`, and the proven
/// `.md` wins on any divergence (see [`load_baseline_model`]).
pub fn mps_enabled() -> bool {
    match std::env::var("AGENT_DOC_MPS") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Path of the structured-overlay baseline sidecar:
/// `<project_root>/.agent-doc/baselines/<hash>.overlay.yrs`.
pub fn baseline_overlay_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root
        .join(BASELINE_DIR)
        .join(format!("{}.{}", hash, BASELINE_OVERLAY_EXT)))
}

/// `#mps` Rung 2 (pin): persist `content` as the model baseline (overlay sidecar)
/// for this cycle, overwriting any prior cycle's. Emits `mps_baseline_pin`. The
/// caller keeps the `.md` baseline; a failure here is returned (and logged by the
/// caller) but does not by itself break the cycle.
pub fn save_baseline_model(doc: &Path, content: &str) -> Result<()> {
    let path = baseline_overlay_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = agent_doc_markdown_ast::crdt::OverlayCrdtDoc::from_markdown(content).encode_state();
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, &state)
        .with_context(|| "failed to write baseline overlay temp file")?;
    tmp.persist(&path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    crate::ops_log::log_op(
        doc,
        &format!(
            "mps_baseline_pin file={} content_len={} overlay_bytes={}",
            doc.display(),
            content.len(),
            state.len()
        ),
    );
    Ok(())
}

/// `#mps` Rung 3 (flip): project the model baseline back to markdown and decide
/// the authoritative merge base, cross-checking against `md_baseline` (the legacy
/// `.md` content, if present). Returns the base the finalize merge should use:
///
/// - `None` — no overlay sidecar pinned; the caller uses the `.md` baseline.
/// - `Some(projection)` — the overlay projected and **agrees** with the `.md` (or
///   there is no `.md`); the model projection is authoritative.
/// - `Some(md)` — the overlay projected but **diverges** from the `.md`. Until
///   Rung 4 removes the `.md`, it is the proven cross-check cache, so it wins as
///   the non-regressing backstop and the divergence is logged loudly
///   (`mps_baseline_divergence` with the first differing byte) for offline diff.
///
/// Defaulting the cutover on (see [`mps_enabled`]) is safe precisely because this
/// can never change a merge outcome while the `.md` exists: it returns the model
/// projection only when it is byte-identical to the legacy base.
pub fn load_baseline_model(doc: &Path, md_baseline: Option<&str>) -> Result<Option<String>> {
    let path = baseline_overlay_path_for(doc)?;
    let bytes = match crate::fs_util::read_optional_bytes(&path)
        .with_context(|| format!("failed to read baseline overlay {}", path.display()))?
    {
        Some(b) => b,
        None => return Ok(None),
    };
    let overlay =
        agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state_or_migrate(&bytes, md_baseline)
            .with_context(|| format!("failed to decode baseline overlay {}", path.display()))?;
    let projection = overlay
        .to_markdown()
        .with_context(|| format!("failed to project baseline overlay {}", path.display()))?;
    let md_len = md_baseline.map(|m| m.len()).unwrap_or(0);

    match md_baseline {
        // `.md` present and diverges — prefer the proven `.md` backstop, log loudly.
        Some(md) if md != projection => {
            crate::ops_log::log_op(
                doc,
                &format!(
                    "mps_baseline_resolve source=md_backstop file={} projected_len={} md_len={} diverged=true",
                    doc.display(),
                    projection.len(),
                    md_len,
                ),
            );
            crate::ops_log::log_op(
                doc,
                &format!(
                    "mps_baseline_divergence file={} projected_len={} md_len={} first_diff_byte={:?}",
                    doc.display(),
                    projection.len(),
                    md_len,
                    first_diff_byte(md, &projection)
                ),
            );
            Ok(Some(md.to_string()))
        }
        // `.md` agrees, or there is no `.md` — the model projection is authoritative.
        _ => {
            crate::ops_log::log_op(
                doc,
                &format!(
                    "mps_baseline_resolve source=model file={} projected_len={} md_len={} diverged=false",
                    doc.display(),
                    projection.len(),
                    md_len,
                ),
            );
            Ok(Some(projection))
        }
    }
}

/// Byte offset of the first difference between `a` and `b` (or the shorter
/// length when one is a prefix of the other). `None` when equal.
fn first_diff_byte(a: &str, b: &str) -> Option<usize> {
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .position(|(x, y)| x != y)
        .or(if a.len() == b.len() {
            None
        } else {
            Some(a.len().min(b.len()))
        })
}

/// Delete the model baseline sidecar for a document, if present. Idempotent.
pub fn delete_baseline_model(doc: &Path) -> Result<()> {
    let path = baseline_overlay_path_for(doc)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Delete the snapshot for a document.
pub fn delete(doc: &Path) -> Result<()> {
    let snap = path_for(doc)?;
    if !snap.exists() {
        return Ok(());
    }
    let _lock = SnapshotLock::acquire(doc)?;
    if snap.exists() {
        std::fs::remove_file(&snap)?;
    }
    Ok(())
}

/// Resolve the best snapshot content for diff computation.
///
/// The snapshot file is always authoritative when it exists — it records the
/// exact baseline written by `agent-doc write` / `submit`, excluding concurrent
/// user edits. Git is only used as a recovery fallback when no snapshot file
/// exists (e.g., first submit after cloning, or snapshot was deleted).
pub fn resolve(doc: &Path) -> Result<Option<String>> {
    let snap_path = path_for(doc)?;
    if snap_path.exists() {
        // Snapshot file exists — always use it (authoritative baseline)
        return load(doc);
    }

    // No snapshot file — try git as recovery fallback.
    // Only useful when git HEAD differs from current file (real recovery).
    // If they match, the file was likely just committed (step 0b) before the
    // first diff — no useful baseline exists.
    let git_mtime = crate::git::last_commit_mtime(doc).unwrap_or(None);
    if git_mtime.is_some() {
        match crate::git::show_head(doc)? {
            Some(git_content) => {
                let current = std::fs::read_to_string(doc).unwrap_or_default();
                if git_content == current {
                    eprintln!(
                        "[snapshot] No snapshot file, git matches current — treating as first submit"
                    );
                    Ok(None)
                } else {
                    eprintln!("[snapshot] No snapshot file, recovering from git");
                    Ok(Some(git_content))
                }
            }
            None => Ok(None),
        }
    } else {
        // First submit — no previous state
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Auto-migration on file rename
// ---------------------------------------------------------------------------

/// Detect and migrate orphaned state files after a document rename.
///
/// When a document is moved/renamed, its path hash changes, orphaning all
/// `.agent-doc/` state files (snapshots, baselines, locks, pending, crdt,
/// pre-response). This function detects the situation by:
/// 1. Checking the document has a session UUID
/// 2. Checking no snapshot exists for the current path hash
/// 3. Scanning existing snapshots for one with a matching session UUID
///
/// If found, migrates all state files from old hash to new hash and updates
/// the sessions registry. Returns `true` if migration occurred.
pub fn try_migrate_renamed(doc: &Path) -> Result<bool> {
    let snap = path_for(doc)?;
    if snap.exists() {
        return Ok(false);
    }

    let content = match std::fs::read_to_string(doc) {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };
    let (fm, _) = match crate::frontmatter::parse(&content) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(false),
    };
    let session_uuid = match &fm.session {
        Some(uuid) => uuid.clone(),
        None => return Ok(false),
    };

    let canonical = doc.canonicalize()?;
    let project_root = match find_project_root(&canonical) {
        Some(root) => root,
        None => return Ok(false),
    };

    let snap_dir = project_root.join(SNAP_DIR);
    if !snap_dir.is_dir() {
        return Ok(false);
    }

    // Scan snapshots for one with matching session UUID
    let mut old_hash: Option<String> = None;
    for entry in std::fs::read_dir(&snap_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Skip if this is the current hash (shouldn't exist, but guard)
        let new_hash = doc_hash(doc)?;
        if stem == new_hash {
            continue;
        }
        // Read and parse frontmatter
        if let Ok(snap_content) = std::fs::read_to_string(&path)
            && let Ok((snap_fm, _)) = crate::frontmatter::parse(&snap_content)
            && snap_fm.session.as_deref() == Some(&session_uuid)
        {
            old_hash = Some(stem);
            break;
        }
    }

    let old_hash = match old_hash {
        Some(h) => h,
        None => return Ok(false),
    };

    let new_hash = doc_hash(doc)?;
    eprintln!(
        "[init] detected rename — migrating state files from {}.. to {}..",
        &old_hash[..8.min(old_hash.len())],
        &new_hash[..8.min(new_hash.len())]
    );

    // Migrate all state file types (same as rename.rs + baselines)
    const MIGRATE_DIRS: &[(&str, &str)] = &[
        ("snapshots", "md"),
        ("baselines", "md"),
        ("baselines", "overlay.yrs"), // #mps model baseline sidecar
        ("locks", "lock"),
        ("pending", "md"),
        ("crdt", "yrs"),
        ("crdt", "overlay.yrs"),
        ("pre-response", "md"),
    ];

    let mut migrated = 0u32;
    for &(subdir, ext) in MIGRATE_DIRS {
        let dir = project_root.join(".agent-doc").join(subdir);
        let old_file = dir.join(format!("{}.{}", old_hash, ext));
        let new_file = dir.join(format!("{}.{}", new_hash, ext));
        if !old_file.exists() {
            continue;
        }
        if new_file.exists() {
            eprintln!(
                "[init] skip migrate {}/{}.{} — destination exists",
                subdir,
                &new_hash[..8.min(new_hash.len())],
                ext
            );
            continue;
        }
        std::fs::rename(&old_file, &new_file)?;
        eprintln!(
            "[init] migrated {}/{}.{} → {}.{}",
            subdir,
            &old_hash[..8.min(old_hash.len())],
            ext,
            &new_hash[..8.min(new_hash.len())],
            ext
        );
        migrated += 1;
    }

    // Migrate lock files with compound extensions
    for (subdir, old_ext) in &[
        ("locks", format!("{}.md.lock", old_hash)),
        ("crdt", format!("{}.yrs.lock", old_hash)),
    ] {
        let dir = project_root.join(".agent-doc").join(subdir);
        let old_file = dir.join(old_ext);
        let new_ext = old_ext.replace(&old_hash, &new_hash);
        let new_file = dir.join(&new_ext);
        if old_file.exists() && !new_file.exists() {
            std::fs::rename(&old_file, &new_file)?;
            migrated += 1;
        }
    }

    // Update sessions registry
    let doc_path_str = doc.to_string_lossy().to_string();
    let canonical_str = canonical.to_string_lossy().to_string();
    let registry_path = crate::sessions::registry_path();
    if registry_path.exists()
        && let Ok(_lock) = crate::sessions::RegistryLock::acquire(&registry_path)
        && let Ok(mut registry) = crate::sessions::load()
    {
        let mut updated = 0u32;
        for entry in registry.values_mut() {
            if entry.session_id == session_uuid
                && entry.file != doc_path_str
                && entry.file != canonical_str
            {
                entry.file = doc_path_str.clone();
                updated += 1;
            }
        }
        if updated > 0 {
            let _ = crate::sessions::save(&registry);
            eprintln!("[init] updated {} session registry entry(ies)", updated);
        }
    }

    eprintln!(
        "[init] rename migration complete — {} state file(s) migrated",
        migrated
    );
    Ok(true)
}

// ---------------------------------------------------------------------------
// Auto-initialization for new documents
// ---------------------------------------------------------------------------

/// Perform **Initialization** for a document entering the agent-doc lifecycle.
///
/// **Ontology:** Initialization ensures a document has all prerequisites for
/// participation in the pane lifecycle: a session UUID (for Binding/Reconciliation),
/// a snapshot (for diff computation), and git tracking (for the gutter boundary).
///
/// Called from `claim`, `preflight`, and `sync`'s `resolve_file` — the three
/// entrypoints where a file first enters agent-doc's awareness.
///
/// Steps:
/// 1. **ensure_session_uuid** — if file has `agent_doc_format` but no `agent_doc_session`, generate and write a UUID
/// 2. **ensure_snapshot** — if no snapshot exists, create one with stripped exchange content
/// 3. **ensure_git_tracked** — `git add` if untracked, then `git commit` for baseline
///
/// Returns `true` if any initialization was performed, `false` if already initialized.
pub fn ensure_initialized(doc: &Path) -> Result<bool> {
    let uuid_assigned = ensure_session_uuid(doc)?;
    let migrated = try_migrate_renamed(doc)?;
    let snapshot_created = if migrated {
        false
    } else {
        ensure_snapshot(doc)?
    };
    if snapshot_created {
        ensure_git_tracked(doc)?;
    }
    Ok(uuid_assigned || migrated || snapshot_created)
}

/// Assign a session UUID to a document that has `agent_doc_format` but no `agent_doc_session`.
///
/// Returns `true` if a UUID was assigned, `false` if already present or not applicable.
pub fn ensure_session_uuid(doc: &Path) -> Result<bool> {
    let content = match std::fs::read_to_string(doc) {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };
    let (fm, _) = match crate::frontmatter::parse(&content) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(false),
    };
    if fm.format.is_none() || fm.session.is_some() {
        return Ok(false);
    }
    eprintln!(
        "[init] assigning session UUID to {} (has format but no session)",
        doc.display()
    );
    let (updated, session_id) = crate::frontmatter::ensure_session(&content)?;
    if updated != content {
        std::fs::write(doc, &updated)?;
        eprintln!("[init] assigned session UUID: {}", session_id);
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Create an initial snapshot with stripped exchange content if none exists.
///
/// Returns `true` if a snapshot was created, `false` if one already existed.
pub fn ensure_snapshot(doc: &Path) -> Result<bool> {
    let snap = path_for(doc)?;
    if snap.exists() {
        return Ok(false);
    }
    eprintln!(
        "[init] creating snapshot for {} (none found)",
        doc.display()
    );
    if let Ok(content) = std::fs::read_to_string(doc) {
        let snapshot_content = crate::claim::strip_exchange_content(&content);
        save(doc, &snapshot_content)?;
    }
    Ok(true)
}

/// Stage an untracked file and commit to establish a git baseline.
pub fn ensure_git_tracked(doc: &Path) -> Result<()> {
    let canonical = std::fs::canonicalize(doc).unwrap_or_else(|_| doc.to_path_buf());
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());

    let is_tracked = std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch"])
        .arg(&canonical)
        .current_dir(&project_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !is_tracked {
        eprintln!("[init] file is untracked — staging with git add");
        let _ = std::process::Command::new("git")
            .args(["add", "--"])
            .arg(&canonical)
            .current_dir(&project_root)
            .status();
    }

    if let Err(e) = crate::git::commit(doc) {
        eprintln!("[init] warning: failed to commit after init: {}", e);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal unlocked helpers (caller must hold SnapshotLock)
// ---------------------------------------------------------------------------

fn load_unlocked(doc: &Path) -> Result<Option<String>> {
    let snap = path_for(doc)?;
    if snap.exists() {
        Ok(Some(std::fs::read_to_string(&snap)?))
    } else {
        Ok(None)
    }
}

fn save_unlocked(doc: &Path, content: &str) -> Result<()> {
    let snap = path_for(doc)?;
    if let Some(parent) = snap.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Atomic write: temp file + rename to avoid partial reads.
    // Redact known secret shapes (API keys, AWS access keys, etc.) before
    // persistence so `.agent-doc/snapshots/<hash>.md` never carries plaintext
    // tokens. `passage open …` invocations are exempt — see secret_redact.
    let redacted = crate::secret_redact::redact(content);
    let parent = snap.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, redacted.as_bytes())
        .with_context(|| "failed to write snapshot temp file")?;
    tmp.persist(&snap)
        .with_context(|| format!("failed to rename temp file to {}", snap.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Pre-response snapshot (for undo/extract)
// ---------------------------------------------------------------------------

const PRE_RESPONSE_DIR: &str = ".agent-doc/pre-response";

/// Compute the pre-response snapshot path for a given document.
/// Returns `<project_root>/.agent-doc/pre-response/<hash>.md`.
pub fn pre_response_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root
        .join(PRE_RESPONSE_DIR)
        .join(format!("{}.md", hash)))
}

/// Save the pre-response snapshot (the document content before the agent's response).
/// Called by write paths before applying patches/appending response.
pub fn save_pre_response(doc: &Path, content: &str) -> Result<()> {
    let path = pre_response_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let redacted = crate::secret_redact::redact(content);
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, redacted.as_bytes())
        .with_context(|| "failed to write pre-response temp file")?;
    tmp.persist(&path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    eprintln!(
        "[snapshot] saved pre-response snapshot for {}",
        doc.display()
    );
    Ok(())
}

/// Load the pre-response snapshot for a document.
pub fn load_pre_response(doc: &Path) -> Result<Option<String>> {
    let path = pre_response_path_for(doc)?;
    crate::fs_util::read_optional_text(&path)
}

/// Delete the pre-response snapshot for a document.
pub fn delete_pre_response(doc: &Path) -> Result<()> {
    let path = pre_response_path_for(doc)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CRDT state persistence (for stream mode)
// ---------------------------------------------------------------------------

/// Compute the legacy text CRDT state file path for a given document.
/// Returns `<project_root>/.agent-doc/crdt/<hash>.yrs`.
/// Falls back to doc's parent directory if no project root found.
pub fn crdt_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let filename = format!("{}.yrs", hash);
    crdt_path_for_filename(doc, filename)
}

/// Compute the structured markdown-overlay CRDT state file path for a document.
/// Returns `<project_root>/.agent-doc/crdt/<hash>.overlay.yrs`.
pub fn overlay_crdt_path_for(doc: &Path) -> Result<PathBuf> {
    let hash = doc_hash(doc)?;
    let filename = format!("{}.overlay.yrs", hash);
    crdt_path_for_filename(doc, filename)
}

fn crdt_path_for_filename(doc: &Path, filename: String) -> Result<PathBuf> {
    let canonical = doc.canonicalize()?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root.join(CRDT_DIR).join(filename))
}

/// Load CRDT state bytes for a document (if any).
pub fn load_crdt(doc: &Path) -> Result<Option<Vec<u8>>> {
    let path = crdt_path_for(doc)?;
    let _lock = acquire_crdt_lock(doc)?;
    crate::fs_util::read_optional_bytes(&path)
        .with_context(|| format!("failed to read CRDT state {}", path.display()))
}

/// Load structured markdown-overlay CRDT state bytes for a document (if any).
pub fn load_overlay_crdt(doc: &Path) -> Result<Option<Vec<u8>>> {
    let path = overlay_crdt_path_for(doc)?;
    let _lock = acquire_crdt_lock(doc)?;
    crate::fs_util::read_optional_bytes(&path)
        .with_context(|| format!("failed to read overlay CRDT state {}", path.display()))
}

/// Save CRDT state bytes for a document.
pub fn save_crdt(doc: &Path, state: &[u8]) -> Result<()> {
    let path = crdt_path_for(doc)?;
    let _lock = acquire_crdt_lock(doc)?;
    write_crdt_state(&path, state)
}

/// Save structured markdown-overlay CRDT state bytes for a document.
pub fn save_overlay_crdt(doc: &Path, state: &[u8]) -> Result<()> {
    let path = overlay_crdt_path_for(doc)?;
    let _lock = acquire_crdt_lock(doc)?;
    write_crdt_state(&path, state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrdtMergeBaseSource {
    Overlay,
    FallbackNoOverlay,
    FallbackOverlayDecodeError,
    FallbackOverlayProjectionMismatch,
}

impl CrdtMergeBaseSource {
    pub fn as_str(self) -> &'static str {
        match self {
            CrdtMergeBaseSource::Overlay => "overlay",
            CrdtMergeBaseSource::FallbackNoOverlay => "fallback_no_overlay",
            CrdtMergeBaseSource::FallbackOverlayDecodeError => "fallback_overlay_decode_error",
            CrdtMergeBaseSource::FallbackOverlayProjectionMismatch => {
                "fallback_overlay_projection_mismatch"
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CrdtMergeBase {
    pub state: Vec<u8>,
    pub source: CrdtMergeBaseSource,
}

/// Build the CRDT merge base for a write cycle.
///
/// The structured overlay sidecar is authoritative when it decodes and its
/// markdown projection matches the explicit cycle baseline. If the overlay is
/// absent or stale, use the known baseline text; using arbitrary stale CRDT
/// sidecars here can replay old content as a fresh concurrent insertion.
pub fn crdt_merge_base_state(doc: &Path, fallback_markdown: &str) -> Result<CrdtMergeBase> {
    let path = overlay_crdt_path_for(doc)?;
    let _lock = acquire_crdt_lock(doc)?;
    let overlay_bytes = crate::fs_util::read_optional_bytes(&path)
        .with_context(|| format!("failed to read overlay CRDT state {}", path.display()))?;

    let (markdown, source) = match overlay_bytes {
        None => (
            fallback_markdown.to_string(),
            CrdtMergeBaseSource::FallbackNoOverlay,
        ),
        Some(bytes) => match agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state_or_migrate(
            &bytes,
            Some(fallback_markdown),
        )
        .and_then(|overlay| overlay.to_markdown())
        {
            Ok(markdown) if markdown == fallback_markdown => {
                (markdown, CrdtMergeBaseSource::Overlay)
            }
            Ok(markdown) => {
                crate::ops_log::log_op(
                    doc,
                    &format!(
                        "crdt_merge_base_overlay_stale file={} overlay_len={} fallback_len={}",
                        doc.display(),
                        markdown.len(),
                        fallback_markdown.len()
                    ),
                );
                (
                    fallback_markdown.to_string(),
                    CrdtMergeBaseSource::FallbackOverlayProjectionMismatch,
                )
            }
            Err(err) => {
                crate::ops_log::log_op(
                    doc,
                    &format!(
                        "crdt_merge_base_overlay_decode_error file={} error={}",
                        doc.display(),
                        err
                    ),
                );
                (
                    fallback_markdown.to_string(),
                    CrdtMergeBaseSource::FallbackOverlayDecodeError,
                )
            }
        },
    };

    crate::ops_log::log_op(
        doc,
        &format!(
            "crdt_merge_base file={} source={} len={}",
            doc.display(),
            source.as_str(),
            markdown.len()
        ),
    );

    Ok(CrdtMergeBase {
        state: crate::crdt::CrdtDoc::from_text(&markdown).encode_state(),
        source,
    })
}

/// Persist both CRDT runtime states for the same markdown snapshot.
///
/// The overlay CRDT is the authoritative structured merge-base sidecar when its
/// markdown projection matches the active cycle baseline. The legacy text CRDT
/// is still persisted for older binaries and editor plugins.
pub fn save_document_crdt(doc: &Path, legacy_state: &[u8], markdown: &str) -> Result<()> {
    save_crdt(doc, legacy_state)?;
    let overlay_state =
        agent_doc_markdown_ast::crdt::OverlayCrdtDoc::from_markdown(markdown).encode_state();
    save_overlay_crdt(doc, &overlay_state)?;
    Ok(())
}

fn write_crdt_state(path: &Path, state: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, state)
        .with_context(|| "failed to write CRDT state temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    Ok(())
}

/// Delete CRDT state for a document.
pub fn delete_crdt(doc: &Path) -> Result<()> {
    let path = crdt_path_for(doc)?;
    let overlay_path = overlay_crdt_path_for(doc)?;
    if path.exists() || overlay_path.exists() {
        let _lock = acquire_crdt_lock(doc)?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        if overlay_path.exists() {
            std::fs::remove_file(&overlay_path)?;
        }
    }
    Ok(())
}

/// Acquire an advisory lock for CRDT state operations.
/// Uses a lock file adjacent to the CRDT state file.
/// Stale lock files (>1 hour old) are cleaned before acquiring.
fn acquire_crdt_lock(doc: &Path) -> Result<File> {
    let crdt_path = crdt_path_for(doc)?;
    let lock_path = crdt_path.with_extension("yrs.lock");
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Clean stale lock file (>1 hour old, from crashed processes)
    if let Ok(meta) = std::fs::metadata(&lock_path)
        && let Some(age) = meta.modified().ok().and_then(|t| t.elapsed().ok())
        && age > std::time::Duration::from_secs(3600)
    {
        let _ = std::fs::remove_file(&lock_path);
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open CRDT lock {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("failed to acquire CRDT lock on {}", lock_path.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "# Test\n").unwrap();
        (dir, doc)
    }

    /// Helper: write a snapshot file directly (without changing CWD).
    fn write_snapshot_directly(dir: &Path, doc: &Path, content: &str) {
        let snap = snapshot_path_in(dir, doc);
        fs::create_dir_all(snap.parent().unwrap()).unwrap();
        fs::write(&snap, content).unwrap();
    }

    /// Helper: read a snapshot file directly (without changing CWD).
    fn read_snapshot_directly(dir: &Path, doc: &Path) -> Option<String> {
        let snap = snapshot_path_in(dir, doc);
        if snap.exists() {
            Some(fs::read_to_string(&snap).unwrap())
        } else {
            None
        }
    }

    /// Compute snapshot path within a specific directory.
    /// If path_for returns absolute (project root found), use it directly.
    /// Otherwise, join relative path with dir.
    fn snapshot_path_in(dir: &Path, doc: &Path) -> PathBuf {
        let p = path_for(doc).unwrap();
        if p.is_absolute() { p } else { dir.join(&p) }
    }

    #[test]
    fn path_for_consistent_hash() {
        let (_dir, doc) = setup();
        let p1 = path_for(&doc).unwrap();
        let p2 = path_for(&doc).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn path_for_different_files_different_hashes() {
        let dir = TempDir::new().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        fs::write(&doc_a, "a").unwrap();
        fs::write(&doc_b, "b").unwrap();
        let pa = path_for(&doc_a).unwrap();
        let pb = path_for(&doc_b).unwrap();
        assert_ne!(pa, pb);
    }

    #[test]
    fn path_for_has_correct_structure() {
        let (_dir, doc) = setup();
        let p = path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/snapshots/"));
        assert!(p.to_string_lossy().ends_with(".md"));
        // Hash is 64 hex chars
        let filename = p.file_stem().unwrap().to_string_lossy();
        assert_eq!(filename.len(), 64);
        assert!(filename.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn load_returns_none_when_no_snapshot() {
        let (_dir, doc) = setup();
        let result = load(&doc).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn snapshot_write_and_read_directly() {
        let (dir, doc) = setup();
        let content = "# Snapshot content\n\nWith body.\n";
        write_snapshot_directly(dir.path(), &doc, content);
        let loaded = read_snapshot_directly(dir.path(), &doc);
        assert_eq!(loaded.as_deref(), Some(content));
    }

    #[test]
    fn snapshot_overwrite() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "first");
        write_snapshot_directly(dir.path(), &doc, "second");
        let loaded = read_snapshot_directly(dir.path(), &doc);
        assert_eq!(loaded.as_deref(), Some("second"));
    }

    #[test]
    fn snapshot_delete_by_removing_file() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "content");
        assert!(read_snapshot_directly(dir.path(), &doc).is_some());

        let snap = snapshot_path_in(dir.path(), &doc);
        fs::remove_file(&snap).unwrap();
        assert!(read_snapshot_directly(dir.path(), &doc).is_none());
    }

    #[test]
    fn delete_no_error_when_missing() {
        let (_dir, doc) = setup();
        delete(&doc).unwrap();
    }

    // -----------------------------------------------------------------------
    // Race condition tests
    // -----------------------------------------------------------------------

    /// Test that flock-based locking works: acquire, hold, release on drop.
    /// Uses raw fs2 flock to avoid SnapshotLock's dependency on path_for/CWD.
    #[test]
    fn flock_acquire_and_release_on_drop() {
        use fs2::FileExt;
        use std::fs::OpenOptions;

        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");

        // First acquire succeeds
        {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(&lock_path)
                .unwrap();
            file.lock_exclusive().unwrap();
            // Lock held here
            file.unlock().unwrap();
        }

        // After drop/unlock, second acquire succeeds
        let file2 = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        file2.lock_exclusive().unwrap();
        file2.unlock().unwrap();
    }

    /// Test that concurrent flock acquisitions serialize properly
    /// (no data loss when multiple threads write through locks).
    #[test]
    fn flock_serializes_concurrent_access() {
        use fs2::FileExt;
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let lock_path = dir.path().join("test.lock");
        let data_path = dir.path().join("data.txt");
        fs::write(&data_path, "0").unwrap();

        let n = 10usize;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for _ in 0..n {
            let lp = lock_path.clone();
            let dp = data_path.clone();
            let bar = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&lp)
                    .unwrap();
                file.lock_exclusive().unwrap();
                // Read-modify-write under lock
                let val: usize = fs::read_to_string(&dp).unwrap().trim().parse().unwrap();
                fs::write(&dp, (val + 1).to_string()).unwrap();
                file.unlock().unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_val: usize = fs::read_to_string(&data_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_eq!(final_val, n, "all {} increments should be serialized", n);
    }

    #[test]
    fn atomic_write_via_tempfile_produces_correct_content() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("output.md");

        // Atomic write: tempfile + persist
        let parent = dir.path();
        let mut tmp = tempfile::NamedTempFile::new_in(parent).unwrap();
        std::io::Write::write_all(&mut tmp, b"atomic content").unwrap();
        tmp.persist(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "atomic content");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("output.md");
        fs::write(&target, "old").unwrap();

        let mut tmp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        std::io::Write::write_all(&mut tmp, b"new").unwrap();
        tmp.persist(&target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn crdt_path_has_correct_extension() {
        let (_dir, doc) = setup();
        let p = crdt_path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/crdt/"));
        assert!(p.to_string_lossy().ends_with(".yrs"));
    }

    #[test]
    fn overlay_crdt_path_has_correct_extension() {
        let (_dir, doc) = setup();
        let p = overlay_crdt_path_for(&doc).unwrap();
        assert!(p.to_string_lossy().contains(".agent-doc/crdt/"));
        assert!(p.to_string_lossy().ends_with(".overlay.yrs"));
    }

    #[test]
    fn crdt_save_and_load_roundtrip() {
        let (_dir, doc) = setup();
        let state = vec![1u8, 2, 3, 4, 5];
        save_crdt(&doc, &state).unwrap();
        let loaded = load_crdt(&doc).unwrap();
        assert_eq!(loaded, Some(state));
    }

    #[test]
    fn overlay_crdt_save_and_load_roundtrip() {
        let (_dir, doc) = setup();
        let state = vec![5u8, 4, 3, 2, 1];
        save_overlay_crdt(&doc, &state).unwrap();
        let loaded = load_overlay_crdt(&doc).unwrap();
        assert_eq!(loaded, Some(state));
    }

    #[test]
    fn document_crdt_save_persists_legacy_and_overlay_state() {
        let (_dir, doc) = setup();
        let markdown = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#first]\n",
            "<!-- /agent:queue -->\n"
        );
        let legacy = crate::crdt::CrdtDoc::from_text(markdown).encode_state();

        save_document_crdt(&doc, &legacy, markdown).unwrap();

        let loaded_legacy = load_crdt(&doc).unwrap().unwrap();
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&loaded_legacy)
                .unwrap()
                .to_text(),
            markdown
        );
        let loaded_overlay = load_overlay_crdt(&doc).unwrap().unwrap();
        let overlay =
            agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state(&loaded_overlay).unwrap();
        assert_eq!(overlay.to_markdown().unwrap(), markdown);
        let components = overlay.to_components().unwrap();
        assert!(components.iter().any(|component| component.name == "queue"));
    }

    #[test]
    fn crdt_merge_base_state_prefers_matching_overlay_projection() {
        let (_dir, doc) = setup();
        let markdown = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#overlay-base]\n",
            "<!-- /agent:queue -->\n"
        );
        let stale_legacy = crate::crdt::CrdtDoc::from_text("stale legacy text").encode_state();
        save_document_crdt(&doc, &stale_legacy, markdown).unwrap();

        let base = crdt_merge_base_state(&doc, markdown).unwrap();

        assert_eq!(base.source, CrdtMergeBaseSource::Overlay);
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&base.state)
                .unwrap()
                .to_text(),
            markdown
        );
    }

    #[test]
    fn crdt_merge_base_state_falls_back_when_overlay_projection_is_stale() {
        let (_dir, doc) = setup();
        let baseline = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#baseline]\n",
            "<!-- /agent:queue -->\n"
        );
        let stale_overlay = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#stale]\n",
            "<!-- /agent:queue -->\n"
        );
        let overlay_state =
            agent_doc_markdown_ast::crdt::OverlayCrdtDoc::from_markdown(stale_overlay)
                .encode_state();
        save_overlay_crdt(&doc, &overlay_state).unwrap();

        let base = crdt_merge_base_state(&doc, baseline).unwrap();

        assert_eq!(
            base.source,
            CrdtMergeBaseSource::FallbackOverlayProjectionMismatch
        );
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&base.state)
                .unwrap()
                .to_text(),
            baseline
        );
    }

    /// #ipc-drift-order-stable-merge: the overlay-as-merge-base path (suspect
    /// 5fd64b26) must stay order-stable for the append case. With a prior
    /// committed `### Re:` response in the baseline, a foreign tail append
    /// landing during generation must not reverse the new response's lines or
    /// hoist it above the prior committed response. This drives the real
    /// `crdt_merge_base_state` overlay path end-to-end (not a hand-built
    /// `from_text` base), so a non-byte-stable overlay projection of an
    /// exchange-with-response document is caught here.
    #[test]
    fn overlay_merge_base_is_order_stable_for_exchange_append() {
        let (_dir, doc) = setup();
        let header = "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n";
        let exchange_committed = "\
❯ first question

### Re: first question — opus-4-8

First answer here. Already committed to HEAD.

❯ second question
";
        let queue_open = "<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n";
        let queue_close = "<!-- /agent:queue -->\n";

        // Baseline: prior response committed, new prompt typed, boundary at tail.
        let base_markdown = format!(
            "{header}{exchange_committed}<!-- agent:boundary:base-id -->\n{queue_open}{queue_close}"
        );

        // Persist the overlay sidecar from the baseline (what a prior cycle saves).
        let stale_legacy = crate::crdt::CrdtDoc::from_text("stale legacy text").encode_state();
        save_document_crdt(&doc, &stale_legacy, &base_markdown).unwrap();

        // The overlay merge base must project to exactly the baseline text — the
        // order-stability guarantee — whether it uses the overlay or falls back.
        let base = crdt_merge_base_state(&doc, &base_markdown).unwrap();
        assert_eq!(
            crate::crdt::CrdtDoc::decode_state(&base.state)
                .unwrap()
                .to_text(),
            base_markdown,
            "overlay merge base is not byte-stable against the baseline (source={})",
            base.source.as_str()
        );

        // Ours: agent replaces the boundary with a new multi-line response.
        let agent_response = "\
### Re: second question — opus-4-8

Second answer line one.
Second answer line two.
Second answer line three.

";
        let ours = format!(
            "{header}{exchange_committed}{agent_response}<!-- agent:boundary:new-id -->\n{queue_open}{queue_close}"
        );
        // Theirs: a foreign supervisor appended a queue item at the tail mid-cycle.
        let theirs = format!(
            "{header}{exchange_committed}<!-- agent:boundary:base-id -->\n{queue_open}- do [#foreign-task]\n{queue_close}"
        );

        let merged = crate::crdt::merge(Some(&base.state), &ours, &theirs).unwrap();

        assert!(
            merged.contains("### Re: second question — opus-4-8"),
            "new response heading dropped under overlay base:\n{merged}"
        );
        let l1 = merged
            .find("Second answer line one.")
            .expect("line one missing");
        let l2 = merged
            .find("Second answer line two.")
            .expect("line two missing");
        let l3 = merged
            .find("Second answer line three.")
            .expect("line three missing");
        assert!(
            l1 < l2 && l2 < l3,
            "response lines reversed under overlay base (l1={l1} l2={l2} l3={l3}):\n{merged}"
        );
        let prior = merged
            .find("### Re: first question")
            .expect("prior response missing");
        let current = merged.find("### Re: second question").unwrap();
        assert!(
            prior < current,
            "new response hoisted above prior HEAD response under overlay base (prior={prior} current={current}):\n{merged}"
        );
        assert!(
            merged.contains("do [#foreign-task]"),
            "foreign queue append lost under overlay base:\n{merged}"
        );
    }

    #[test]
    fn crdt_load_returns_none_when_missing() {
        let (_dir, doc) = setup();
        let loaded = load_crdt(&doc).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn crdt_delete_removes_file() {
        let (_dir, doc) = setup();
        save_crdt(&doc, &[1, 2, 3]).unwrap();
        save_overlay_crdt(&doc, &[4, 5, 6]).unwrap();
        assert!(load_crdt(&doc).unwrap().is_some());
        assert!(load_overlay_crdt(&doc).unwrap().is_some());
        delete_crdt(&doc).unwrap();
        assert!(load_crdt(&doc).unwrap().is_none());
        assert!(load_overlay_crdt(&doc).unwrap().is_none());
    }

    #[test]
    fn crdt_delete_no_error_when_missing() {
        let (_dir, doc) = setup();
        delete_crdt(&doc).unwrap();
    }

    #[test]
    fn concurrent_atomic_writes_no_partial_content() {
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let target = dir.path().join("concurrent.md");
        fs::write(&target, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let path = target.clone();
            let parent = dir.path().to_path_buf();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut tmp = tempfile::NamedTempFile::new_in(&parent).unwrap();
                std::io::Write::write_all(&mut tmp, content.as_bytes()).unwrap();
                tmp.persist(&path).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Final content must be exactly one valid write (no corruption/partial)
        let final_content = fs::read_to_string(&target).unwrap();
        assert!(
            final_content.starts_with("writer-") && final_content.ends_with("-content"),
            "unexpected content: {}",
            final_content
        );
    }

    #[test]
    fn resolve_prefers_snapshot_over_git() {
        // Verify that resolve() uses the snapshot file when it exists,
        // regardless of git commit mtime. This prevents the bug where
        // step 0b commit makes git newer than snapshot, causing resolve()
        // to return git content (= current file) instead of the snapshot.
        let (dir, doc) = setup();
        let snapshot_content = "snapshot baseline content";
        write_snapshot_directly(dir.path(), &doc, snapshot_content);

        // Even though the doc file on disk has different content,
        // resolve should return the snapshot file content.
        let resolved = resolve(&doc).unwrap();
        assert_eq!(
            resolved.as_deref(),
            Some(snapshot_content),
            "resolve() should always prefer snapshot file when it exists"
        );
    }

    // -----------------------------------------------------------------------
    // ensure_session_uuid tests
    // -----------------------------------------------------------------------

    fn setup_with_frontmatter(content: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    #[test]
    fn ensure_session_uuid_assigns_to_template_without_session() {
        let (_dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n",
        );
        let assigned = ensure_session_uuid(&doc).unwrap();
        assert!(
            assigned,
            "should assign UUID to template file without session"
        );

        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("agent_doc_session:"),
            "file should have session UUID"
        );
    }

    #[test]
    fn ensure_session_uuid_noop_when_session_exists() {
        let (_dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_session: existing-uuid\nagent_doc_format: template\n---\n\nBody\n",
        );
        let assigned = ensure_session_uuid(&doc).unwrap();
        assert!(!assigned, "should not reassign when session already exists");

        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("existing-uuid"), "original UUID preserved");
    }

    #[test]
    fn ensure_session_uuid_noop_when_no_format() {
        let (_dir, doc) =
            setup_with_frontmatter("---\ntitle: plain doc\n---\n\nNo agent_doc_format\n");
        let assigned = ensure_session_uuid(&doc).unwrap();
        assert!(!assigned, "should not assign UUID to non-agent-doc files");
    }

    // -----------------------------------------------------------------------
    // ensure_snapshot tests
    // -----------------------------------------------------------------------

    #[test]
    fn ensure_snapshot_creates_when_missing() {
        let (dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_session: test-uuid\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append -->\nuser text\n<!-- /agent:exchange -->\n",
        );
        // Create .agent-doc dir so snapshot path resolves
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();

        let created = ensure_snapshot(&doc).unwrap();
        assert!(created, "should create snapshot when none exists");

        let snap = read_snapshot_directly(dir.path(), &doc);
        assert!(snap.is_some(), "snapshot file should exist");
        // Exchange content should be stripped
        let snap_content = snap.unwrap();
        assert!(
            !snap_content.contains("user text"),
            "snapshot should have stripped exchange content"
        );
    }

    #[test]
    fn ensure_snapshot_noop_when_exists() {
        let (dir, doc) = setup();
        write_snapshot_directly(dir.path(), &doc, "existing snapshot");

        let created = ensure_snapshot(&doc).unwrap();
        assert!(!created, "should not recreate when snapshot exists");

        let snap = read_snapshot_directly(dir.path(), &doc).unwrap();
        assert_eq!(snap, "existing snapshot", "existing snapshot preserved");
    }

    // -----------------------------------------------------------------------
    // ensure_initialized composite tests
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // try_migrate_renamed tests
    // -----------------------------------------------------------------------

    fn setup_project_with_session(session_uuid: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        // Create .agent-doc/ directory structure (project root marker)
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/baselines")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/crdt")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/pre-response")).unwrap();

        let doc = dir.path().join("test.md");
        let content = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\n## Exchange\nSome content\n",
            session_uuid
        );
        fs::write(&doc, &content).unwrap();
        (dir, doc)
    }

    #[test]
    fn try_migrate_renamed_finds_orphaned_snapshot() {
        let session_uuid = "test-uuid-1234";
        let (dir, old_doc) = setup_project_with_session(session_uuid);

        // Save a snapshot for the old path
        let old_hash = doc_hash(&old_doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        let old_snap_content = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\nStripped exchange\n",
            session_uuid
        );
        fs::write(snap_dir.join(format!("{}.md", old_hash)), &old_snap_content).unwrap();

        // Also create a baseline and pending file for the old hash
        let baseline_dir = dir.path().join(".agent-doc/baselines");
        fs::write(
            baseline_dir.join(format!("{}.md", old_hash)),
            "baseline content",
        )
        .unwrap();
        let pending_dir = dir.path().join(".agent-doc/pending");
        fs::write(
            pending_dir.join(format!("{}.md", old_hash)),
            "pending content",
        )
        .unwrap();

        // "Rename" the file
        let new_doc = dir.path().join("renamed.md");
        fs::rename(&old_doc, &new_doc).unwrap();

        // Verify new hash differs
        let new_hash = doc_hash(&new_doc).unwrap();
        assert_ne!(old_hash, new_hash);

        // Run migration
        let migrated = try_migrate_renamed(&new_doc).unwrap();
        assert!(migrated, "should detect and migrate orphaned snapshot");

        // Verify state files were migrated
        assert!(snap_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!snap_dir.join(format!("{}.md", old_hash)).exists());
        assert!(baseline_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!baseline_dir.join(format!("{}.md", old_hash)).exists());
        assert!(pending_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!pending_dir.join(format!("{}.md", old_hash)).exists());

        // Verify snapshot content preserved
        let new_snap = fs::read_to_string(snap_dir.join(format!("{}.md", new_hash))).unwrap();
        assert_eq!(new_snap, old_snap_content);
    }

    #[test]
    fn try_migrate_renamed_noop_when_snapshot_exists() {
        let session_uuid = "test-uuid-5678";
        let (dir, doc) = setup_project_with_session(session_uuid);

        // Save a snapshot for the current path
        let hash = doc_hash(&doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        fs::write(snap_dir.join(format!("{}.md", hash)), "existing snapshot").unwrap();

        let migrated = try_migrate_renamed(&doc).unwrap();
        assert!(!migrated, "should not migrate when snapshot already exists");
    }

    #[test]
    fn try_migrate_renamed_noop_when_no_session() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(
            &doc,
            "---\nagent_doc_format: template\n---\n\nNo session UUID\n",
        )
        .unwrap();

        let migrated = try_migrate_renamed(&doc).unwrap();
        assert!(!migrated, "should not migrate without session UUID");
    }

    #[test]
    fn try_migrate_renamed_noop_when_no_match() {
        let session_uuid = "test-uuid-abcd";
        let (dir, doc) = setup_project_with_session(session_uuid);

        // Create a snapshot with a DIFFERENT session UUID
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        fs::write(
            snap_dir.join("deadbeef1234567890abcdef1234567890abcdef1234567890abcdef12345678.md"),
            "---\nagent_doc_session: different-uuid\nagent_doc_format: template\n---\n\nOther doc\n",
        ).unwrap();

        let migrated = try_migrate_renamed(&doc).unwrap();
        assert!(!migrated, "should not migrate when no matching UUID found");
    }

    #[test]
    fn ensure_initialized_assigns_uuid_even_when_snapshot_exists() {
        let (dir, doc) = setup_with_frontmatter(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\nBody\n",
        );
        // Pre-create snapshot so ensure_snapshot is a no-op
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        write_snapshot_directly(dir.path(), &doc, "pre-existing snapshot");

        let initialized = ensure_initialized(&doc).unwrap();
        assert!(initialized, "should return true when UUID was assigned");

        let content = fs::read_to_string(&doc).unwrap();
        assert!(
            content.contains("agent_doc_session:"),
            "UUID should be assigned"
        );
    }

    /// Simulates the `start` command scenario: file already has a session UUID
    /// (written by `frontmatter::ensure_session`), then is moved. Calling
    /// `ensure_initialized` on the new path should migrate orphaned state.
    #[test]
    fn ensure_initialized_migrates_after_move_with_existing_session() {
        let session_uuid = "start-scenario-uuid-1234";
        let (dir, old_doc) = setup_project_with_session(session_uuid);

        // Create snapshot + CRDT state for old path (simulates prior session)
        let old_hash = doc_hash(&old_doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        let crdt_dir = dir.path().join(".agent-doc/crdt");
        let old_snap = format!(
            "---\nagent_doc_session: {}\nagent_doc_format: template\n---\n\nOld snapshot\n",
            session_uuid
        );
        fs::write(snap_dir.join(format!("{}.md", old_hash)), &old_snap).unwrap();
        fs::write(crdt_dir.join(format!("{}.yrs", old_hash)), b"crdt-state").unwrap();
        fs::write(
            crdt_dir.join(format!("{}.overlay.yrs", old_hash)),
            b"overlay-crdt-state",
        )
        .unwrap();

        // Move the file (simulates JB plugin respawn after rename)
        let new_doc = dir.path().join("moved-doc.md");
        fs::rename(&old_doc, &new_doc).unwrap();
        let new_hash = doc_hash(&new_doc).unwrap();
        assert_ne!(old_hash, new_hash);

        // Call ensure_initialized — the path start.rs now takes
        let initialized = ensure_initialized(&new_doc).unwrap();
        assert!(initialized, "should migrate orphaned state");

        // Snapshot migrated to new hash
        assert!(snap_dir.join(format!("{}.md", new_hash)).exists());
        assert!(!snap_dir.join(format!("{}.md", old_hash)).exists());

        // CRDT state migrated to new hash
        assert!(crdt_dir.join(format!("{}.yrs", new_hash)).exists());
        assert!(!crdt_dir.join(format!("{}.yrs", old_hash)).exists());
        assert!(crdt_dir.join(format!("{}.overlay.yrs", new_hash)).exists());
        assert!(!crdt_dir.join(format!("{}.overlay.yrs", old_hash)).exists());
    }

    /// When `start` calls `ensure_initialized` on a file with no prior state
    /// (no snapshot, no orphaned state), it should bootstrap a fresh snapshot.
    #[test]
    fn ensure_initialized_bootstraps_snapshot_for_fresh_file() {
        let session_uuid = "fresh-start-uuid-5678";
        let (dir, doc) = setup_project_with_session(session_uuid);

        // No snapshot exists — ensure_initialized should create one
        let initialized = ensure_initialized(&doc).unwrap();
        assert!(initialized, "should create snapshot for fresh file");

        // Verify snapshot was created
        let hash = doc_hash(&doc).unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        assert!(
            snap_dir.join(format!("{}.md", hash)).exists(),
            "snapshot should be bootstrapped"
        );
    }

    // -----------------------------------------------------------------------
    // `#mps` Rung 1 — byte-stable projection evals
    //
    // Prove the structured overlay model projects each document shape agent-doc
    // actually persists back to byte-identical markdown through the same
    // encode→decode→to_markdown pipeline the merge base uses. These are the
    // offline half of the #mps obstacle-2 gate; the env-gated `save` probe
    // collects the live-traffic half.
    // -----------------------------------------------------------------------

    fn assert_projection_byte_stable(label: &str, content: &str) {
        let projected = project_overlay_roundtrip(content);
        assert_eq!(
            projected.as_deref(),
            Some(content),
            "#mps overlay projection not byte-stable for {label}"
        );
        assert!(overlay_projection_is_byte_stable(content), "{label}");
    }

    #[test]
    fn mps_projection_byte_stable_inline_shape() {
        let inline = concat!(
            "---\nagent_doc_format: inline\n---\n\n",
            "## User\n\nDo the thing.\n\n",
            "## Assistant\n\nDid the thing.\n"
        );
        assert_projection_byte_stable("inline", inline);
    }

    #[test]
    fn mps_projection_byte_stable_template_queue_shape() {
        let template = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#a]\n- do [#b]\n",
            "<!-- /agent:queue -->\n"
        );
        assert_projection_byte_stable("template_queue", template);
    }

    #[test]
    fn mps_projection_byte_stable_exchange_append_shape() {
        let exchange = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "\u{276f} first question\n\n",
            "### Re: first question \u{2014} opus-4-8\n\n",
            "First answer here.\n\n",
            "\u{276f} second question\n",
            "<!-- /agent:exchange -->\n"
        );
        assert_projection_byte_stable("exchange_append", exchange);
    }

    #[test]
    fn mps_projection_byte_stable_boundary_marker_shape() {
        let with_boundary = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "\u{276f} q\n\n### Re: q \u{2014} opus-4-8\n\nA.\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        assert_projection_byte_stable("boundary_marker", with_boundary);
    }

    #[test]
    fn mps_projection_byte_stable_empty_and_unicode() {
        assert_projection_byte_stable("empty", "");
        assert_projection_byte_stable("plain", "just a line of prose\n");
        assert_projection_byte_stable(
            "unicode",
            "# T\u{e9}l\u{e9} \u{1f680}\n\n- caf\u{e9} \u{2014} r\u{e9}sum\u{e9}\n",
        );
    }

    // -----------------------------------------------------------------------
    // `#mps` Rungs 2-4 — model-projected baseline store
    // -----------------------------------------------------------------------

    #[test]
    fn mps_baseline_model_roundtrips_byte_identical() {
        let (_dir, doc) = setup();
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n"
        );
        save_baseline_model(&doc, baseline).unwrap();
        // Cross-checked against the matching `.md` content: projects identically,
        // no divergence.
        let projected = load_baseline_model(&doc, Some(baseline)).unwrap();
        assert_eq!(projected.as_deref(), Some(baseline));
    }

    #[test]
    fn mps_baseline_model_none_when_absent() {
        let (_dir, doc) = setup();
        // No sidecar pinned → caller falls back to the `.md` baseline.
        assert!(
            load_baseline_model(&doc, Some("anything"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn mps_baseline_model_prefers_md_backstop_on_divergence() {
        let (_dir, doc) = setup();
        let pinned = "## Queue\n<!-- agent:queue -->\n- do [#real]\n<!-- /agent:queue -->\n";
        save_baseline_model(&doc, pinned).unwrap();
        // When the `.md` cross-check cache disagrees with the model projection, the
        // proven `.md` wins as the non-regressing backstop (until Rung 4 removes
        // it); the divergence is logged. This is what makes default-on safe.
        let stale_md = "## Queue\n<!-- agent:queue -->\n- do [#stale]\n<!-- /agent:queue -->\n";
        let resolved = load_baseline_model(&doc, Some(stale_md)).unwrap();
        assert_eq!(resolved.as_deref(), Some(stale_md));
    }

    #[test]
    fn mps_baseline_model_uses_projection_when_no_md() {
        let (_dir, doc) = setup();
        // With no `.md` (the eventual Rung-4 state), the model projection is the
        // only and authoritative base.
        let pinned = "## Queue\n<!-- agent:queue -->\n- do [#only]\n<!-- /agent:queue -->\n";
        save_baseline_model(&doc, pinned).unwrap();
        let resolved = load_baseline_model(&doc, None).unwrap();
        assert_eq!(resolved.as_deref(), Some(pinned));
    }

    #[test]
    fn mps_delete_baseline_model_idempotent() {
        let (_dir, doc) = setup();
        // Delete-before-create is a no-op, not an error.
        delete_baseline_model(&doc).unwrap();
        save_baseline_model(&doc, "x\n").unwrap();
        assert!(baseline_overlay_path_for(&doc).unwrap().exists());
        delete_baseline_model(&doc).unwrap();
        assert!(!baseline_overlay_path_for(&doc).unwrap().exists());
        delete_baseline_model(&doc).unwrap();
    }

    #[test]
    fn mps_first_diff_byte_locates_mismatch() {
        assert_eq!(first_diff_byte("abc", "abc"), None);
        assert_eq!(first_diff_byte("abc", "abx"), Some(2));
        assert_eq!(first_diff_byte("abc", "ab"), Some(2)); // prefix → shorter len
        assert_eq!(first_diff_byte("ab", "abcd"), Some(2));
    }
}

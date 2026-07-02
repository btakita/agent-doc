//! Snapshot sidecar I/O.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use agent_doc_document::model_projection::{
    ModelBaselineSource, overlay_state_from_markdown, project_overlay_roundtrip,
    project_overlay_state, resolve_model_baseline_projection,
};
use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;
use agent_doc_document_realtime::crdt_merge_base::{CrdtMergeBase, resolve_crdt_merge_base};
use agent_doc_frontmatter::frontmatter::session_id_from_content;
use agent_doc_git_io::revision::HeadWorktreeFallback;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotStateMigrationReport {
    pub migrated: u32,
    pub events: Vec<SnapshotStateMigrationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotStateMigrationEvent {
    SkippedDestinationExists {
        subdir: String,
        new_hash_prefix: String,
        ext: String,
    },
    Migrated {
        subdir: String,
        old_hash_prefix: String,
        new_hash_prefix: String,
        ext: String,
    },
}

impl SnapshotStateMigrationEvent {
    pub fn log_message(&self) -> String {
        match self {
            SnapshotStateMigrationEvent::SkippedDestinationExists {
                subdir,
                new_hash_prefix,
                ext,
            } => format!(
                "[init] skip migrate {}/{}.{} — destination exists",
                subdir, new_hash_prefix, ext
            ),
            SnapshotStateMigrationEvent::Migrated {
                subdir,
                old_hash_prefix,
                new_hash_prefix,
                ext,
            } => format!(
                "[init] migrated {}/{}.{} → {}.{}",
                subdir, old_hash_prefix, ext, new_hash_prefix, ext
            ),
        }
    }
}

/// RAII guard for exclusive advisory lock on a markdown snapshot file.
pub struct SnapshotLock {
    _file: File,
    lock_path: PathBuf,
}

impl SnapshotLock {
    /// Acquire an exclusive advisory lock for the snapshot of the given document.
    pub fn acquire(doc: &Path) -> Result<Self> {
        let lock_path = agent_doc_fs::snapshot_flock_path_for(doc)?;
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
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// Load markdown snapshot content under the snapshot lock.
pub fn load(doc: &Path) -> Result<Option<String>> {
    let snap = agent_doc_fs::snapshot_path_for(doc)?;
    if !snap.exists() {
        return Ok(None);
    }
    let _lock = SnapshotLock::acquire(doc)?;
    load_unlocked(doc)
}

/// Save markdown snapshot content under the snapshot lock.
pub fn save(doc: &Path, content: &str, mut logger: impl FnMut(&Path, &str)) -> Result<()> {
    let _lock = SnapshotLock::acquire(doc)?;
    save_unlocked(doc, content)?;
    logger(
        doc,
        &format!("snapshot_save file={} len={}", doc.display(), content.len()),
    );
    probe_overlay_projection(doc, content, logger);
    Ok(())
}

/// Resolve the best markdown snapshot content for diff computation.
///
/// The snapshot file is authoritative when present. Git is only used as a
/// recovery fallback when no snapshot exists and HEAD differs from the current
/// worktree file.
pub fn resolve(doc: &Path) -> Result<Option<String>> {
    let snap_path = agent_doc_fs::snapshot_path_for(doc)?;
    if snap_path.exists() {
        return load(doc);
    }

    match agent_doc_git_io::revision::head_fallback_when_differs_from_worktree(doc)? {
        HeadWorktreeFallback::NoHead => Ok(None),
        HeadWorktreeFallback::MatchesCurrent => {
            eprintln!(
                "[snapshot] No snapshot file, git matches current — treating as first submit"
            );
            Ok(None)
        }
        HeadWorktreeFallback::DiffersFromCurrent(git_content) => {
            eprintln!("[snapshot] No snapshot file, recovering from git");
            Ok(Some(git_content))
        }
    }
}

/// Diff-IO snapshot adapter backed by markdown snapshot sidecars.
pub struct DiffSnapshotStore {
    logger: fn(&Path, &str),
}

impl DiffSnapshotStore {
    pub const fn new(logger: fn(&Path, &str)) -> Self {
        Self { logger }
    }
}

impl agent_doc_diff_io::SnapshotStore for DiffSnapshotStore {
    fn resolve(&self, doc: &Path) -> Result<Option<String>> {
        resolve(doc)
    }

    fn save(&self, doc: &Path, content: &str) -> Result<()> {
        save(doc, content, self.logger)
    }
}

/// Delete a markdown snapshot under the snapshot lock.
pub fn delete(doc: &Path) -> Result<()> {
    let snap = agent_doc_fs::snapshot_path_for(doc)?;
    if !snap.exists() {
        return Ok(());
    }
    let _lock = SnapshotLock::acquire(doc)?;
    if snap.exists() {
        std::fs::remove_file(&snap)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotCommitStatus {
    Committed,
    SnapshotDiffersFromHead {
        snapshot_len: usize,
        head_len: usize,
    },
    NoSnapshot,
    NoHead,
    NotInGitRepo,
}

/// Compare loaded snapshot and HEAD document content, modulo transient markers.
pub fn snapshot_commit_status_from_contents(
    snapshot: Option<&str>,
    head_doc: Option<&str>,
) -> SnapshotCommitStatus {
    let Some(snapshot) = snapshot else {
        return SnapshotCommitStatus::NoSnapshot;
    };
    let Some(head_doc) = head_doc else {
        return SnapshotCommitStatus::NoHead;
    };
    let normalized_snapshot = normalize_transient_agent_doc_markers(snapshot);
    let normalized_head = normalize_transient_agent_doc_markers(head_doc);
    if normalized_snapshot == normalized_head {
        SnapshotCommitStatus::Committed
    } else {
        SnapshotCommitStatus::SnapshotDiffersFromHead {
            snapshot_len: normalized_snapshot.len(),
            head_len: normalized_head.len(),
        }
    }
}

/// Verify that the current markdown snapshot for `file` is committed in its
/// owning git root.
///
/// Compares snapshot content, modulo transient agent-doc markers, against
/// `git show HEAD:<file>` in the narrowed git root. Returns `Committed` when
/// they match, or a specific variant explaining the mismatch.
pub fn verify_snapshot_committed(file: &Path) -> Result<SnapshotCommitStatus> {
    if !agent_doc_git_io::status::is_in_git_repo(file) {
        return Ok(SnapshotCommitStatus::NotInGitRepo);
    }
    let snapshot = load(file)?;
    let head_doc = agent_doc_git_io::revision::show_head(file)?;
    Ok(snapshot_commit_status_from_contents(
        snapshot.as_deref(),
        head_doc.as_deref(),
    ))
}

/// Create the initial markdown snapshot if no snapshot exists yet.
///
/// The caller supplies the content projection so domain-specific stripping or
/// normalization stays outside snapshot IO. This helper owns the file/path
/// existence check, document read, and atomic snapshot save.
pub fn ensure_initial_snapshot(
    doc: &Path,
    project_content: impl FnOnce(&str) -> String,
    logger: impl FnMut(&Path, &str),
) -> Result<bool> {
    let snap = agent_doc_fs::snapshot_path_for(doc)?;
    if snap.exists() {
        return Ok(false);
    }
    eprintln!(
        "[init] creating snapshot for {} (none found)",
        doc.display()
    );
    if let Ok(content) = std::fs::read_to_string(doc) {
        let snapshot_content = project_content(&content);
        save(doc, &snapshot_content, logger)?;
    }
    Ok(true)
}

/// Detect and migrate orphaned state files after a document rename.
///
/// A document rename changes the path-derived state hash. This helper detects
/// an orphaned markdown snapshot carrying the same `agent_doc_session`, moves
/// all matching sidecars to the new hash, and updates the session registry
/// entry for that session.
pub fn try_migrate_renamed(doc: &Path) -> Result<bool> {
    let snap = agent_doc_fs::snapshot_path_for(doc)?;
    if snap.exists() {
        return Ok(false);
    }

    let session_uuid = match std::fs::read_to_string(doc)
        .ok()
        .and_then(|content| session_id_from_content(&content))
    {
        Some(uuid) => uuid,
        None => return Ok(false),
    };

    let canonical = doc.canonicalize()?;
    let project_root = match agent_doc_project_root_io::project_root_containing(&canonical) {
        Some(root) => root,
        None => return Ok(false),
    };

    let snap_dir = snap.parent().unwrap_or(Path::new(".")).to_path_buf();
    if !snap_dir.is_dir() {
        return Ok(false);
    }
    let new_hash = agent_doc_fs::document_state_hash(doc)?;

    let old_hash = match find_snapshot_hash_for_session(&snap_dir, &new_hash, &session_uuid)? {
        Some(h) => h,
        None => return Ok(false),
    };

    eprintln!(
        "[init] detected rename — migrating state files from {}.. to {}..",
        &old_hash[..8.min(old_hash.len())],
        &new_hash[..8.min(new_hash.len())]
    );

    let migration_report = migrate_state_files_for_hash(&project_root, &old_hash, &new_hash)?;
    for event in &migration_report.events {
        eprintln!("{}", event.log_message());
    }

    let updated = agent_doc_session_registry_io::update_session_file_in(
        &project_root,
        &session_uuid,
        doc,
        &canonical,
    )?;
    if updated > 0 {
        eprintln!("[init] updated {} session registry entry(ies)", updated);
    }

    eprintln!(
        "[init] rename migration complete — {} state file(s) migrated",
        migration_report.migrated
    );
    Ok(true)
}

/// Migrate state sidecars from an old document hash to a new document hash.
///
/// The caller owns detecting the matching session and updating any session
/// registry. This helper owns only the concrete sidecar file moves.
pub fn migrate_state_files_for_hash(
    project_root: &Path,
    old_hash: &str,
    new_hash: &str,
) -> Result<SnapshotStateMigrationReport> {
    const MIGRATE_DIRS: &[(&str, &str)] = &[
        ("snapshots", "md"),
        ("baselines", "md"),
        ("baselines", "overlay.yrs"),
        ("locks", "lock"),
        ("pending", "md"),
        ("crdt", "yrs"),
        ("crdt", "overlay.yrs"),
        ("crdt", "nodes.yrs"),
        ("pre-response", "md"),
    ];

    let mut report = SnapshotStateMigrationReport {
        migrated: 0,
        events: Vec::new(),
    };

    for &(subdir, ext) in MIGRATE_DIRS {
        let dir = project_root.join(".agent-doc").join(subdir);
        let old_file = dir.join(format!("{}.{}", old_hash, ext));
        let new_file = dir.join(format!("{}.{}", new_hash, ext));
        if !old_file.exists() {
            continue;
        }
        if new_file.exists() {
            report
                .events
                .push(SnapshotStateMigrationEvent::SkippedDestinationExists {
                    subdir: subdir.to_string(),
                    new_hash_prefix: hash_prefix(new_hash),
                    ext: ext.to_string(),
                });
            continue;
        }
        std::fs::rename(&old_file, &new_file)?;
        report.events.push(SnapshotStateMigrationEvent::Migrated {
            subdir: subdir.to_string(),
            old_hash_prefix: hash_prefix(old_hash),
            new_hash_prefix: hash_prefix(new_hash),
            ext: ext.to_string(),
        });
        report.migrated += 1;
    }

    // Migrate lock files with compound extensions.
    for (subdir, old_ext) in &[
        ("locks", format!("{}.md.lock", old_hash)),
        ("crdt", format!("{}.yrs.lock", old_hash)),
    ] {
        let dir = project_root.join(".agent-doc").join(subdir);
        let old_file = dir.join(old_ext);
        let new_ext = old_ext.replace(old_hash, new_hash);
        let new_file = dir.join(&new_ext);
        if old_file.exists() && !new_file.exists() {
            std::fs::rename(&old_file, &new_file)?;
            report.migrated += 1;
        }
    }

    Ok(report)
}

/// Find an orphaned snapshot hash whose frontmatter carries `session_uuid`.
///
/// The caller owns deciding whether rename migration should run. This helper
/// only scans snapshot sidecars and skips the current document hash.
pub fn find_snapshot_hash_for_session(
    snap_dir: &Path,
    current_hash: &str,
    session_uuid: &str,
) -> Result<Option<String>> {
    if !snap_dir.is_dir() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(snap_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) if stem != current_hash => stem.to_string(),
            _ => continue,
        };
        if let Ok(snapshot_content) = std::fs::read_to_string(&path)
            && session_id_from_content(&snapshot_content).as_deref() == Some(session_uuid)
        {
            return Ok(Some(stem));
        }
    }
    Ok(None)
}

fn hash_prefix(hash: &str) -> String {
    hash[..8.min(hash.len())].to_string()
}

/// Save the pre-response snapshot for undo/extract flows.
pub fn save_pre_response(doc: &Path, content: &str) -> Result<()> {
    let path = agent_doc_fs::pre_response_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let redacted = agent_doc_secret_redact::redact(content);
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
    let path = agent_doc_fs::pre_response_path_for(doc)?;
    agent_doc_fs::read_optional_text(&path)
}

/// Delete the pre-response snapshot for a document.
pub fn delete_pre_response(doc: &Path) -> Result<()> {
    let path = agent_doc_fs::pre_response_path_for(doc)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Run `action` while holding the document's CRDT advisory lock.
pub fn with_crdt_lock<T>(doc: &Path, action: impl FnOnce() -> Result<T>) -> Result<T> {
    let _lock = acquire_crdt_lock(doc)?;
    action()
}

/// Build the CRDT merge base for a write cycle from persisted sidecar state.
///
/// The caller supplies the live editor-op evidence and log sink so snapshot IO
/// owns sidecar resolution without depending on orchestration runtime state.
pub fn crdt_merge_base_state_with(
    doc: &Path,
    fallback_markdown: &str,
    has_pending_editor_ops: impl FnOnce(&Path) -> bool,
    mut logger: impl FnMut(&Path, &str),
) -> Result<CrdtMergeBase> {
    let path = agent_doc_fs::overlay_crdt_path_for(doc)?;
    with_crdt_lock(doc, || {
        let overlay_bytes = read_crdt_state_file(&path, "overlay CRDT state")?;

        let resolution = resolve_crdt_merge_base(
            overlay_bytes.as_deref(),
            fallback_markdown,
            has_pending_editor_ops(doc),
        );

        for event in &resolution.events {
            logger(doc, &event.log_message(doc.display()));
        }
        if resolution.rebuild_overlay_to_baseline {
            rebuild_overlay_to_baseline(doc, &path, fallback_markdown, &mut logger);
        }

        Ok(resolution.base)
    })
}

fn rebuild_overlay_to_baseline(
    doc: &Path,
    path: &Path,
    baseline: &str,
    logger: &mut impl FnMut(&Path, &str),
) {
    match write_overlay_crdt_state_file_from_markdown(path, baseline) {
        Ok(overlay_bytes) => logger(
            doc,
            &format!(
                "crdt_merge_base_overlay_rebuilt file={} fallback_len={} overlay_bytes={}",
                doc.display(),
                baseline.len(),
                overlay_bytes
            ),
        ),
        Err(err) => logger(
            doc,
            &format!(
                "crdt_merge_base_overlay_rebuild_failed file={} error={}",
                doc.display(),
                err
            ),
        ),
    }
}

/// Load legacy text CRDT state bytes for a document, if present.
pub fn load_crdt(doc: &Path) -> Result<Option<Vec<u8>>> {
    with_crdt_lock(doc, || {
        let path = agent_doc_fs::crdt_path_for(doc)?;
        read_crdt_state_file(&path, "CRDT state")
    })
}

/// Load structured markdown-overlay CRDT state bytes for a document, if present.
pub fn load_overlay_crdt(doc: &Path) -> Result<Option<Vec<u8>>> {
    with_crdt_lock(doc, || {
        let path = agent_doc_fs::overlay_crdt_path_for(doc)?;
        read_crdt_state_file(&path, "overlay CRDT state")
    })
}

/// Save legacy text CRDT state bytes for a document.
pub fn save_crdt(doc: &Path, state: &[u8]) -> Result<()> {
    with_crdt_lock(doc, || {
        let path = agent_doc_fs::crdt_path_for(doc)?;
        write_crdt_state_file(&path, state)
    })
}

/// Save structured markdown-overlay CRDT state bytes for a document.
pub fn save_overlay_crdt(doc: &Path, state: &[u8]) -> Result<()> {
    with_crdt_lock(doc, || {
        let path = agent_doc_fs::overlay_crdt_path_for(doc)?;
        write_crdt_state_file(&path, state)
    })
}

/// Encode markdown and save it as structured markdown-overlay CRDT state.
pub fn save_overlay_crdt_from_markdown(doc: &Path, markdown: &str) -> Result<usize> {
    with_crdt_lock(doc, || {
        let path = agent_doc_fs::overlay_crdt_path_for(doc)?;
        write_overlay_crdt_state_file_from_markdown(&path, markdown)
    })
}

/// Load raw per-node CRDT container bytes for a document, if present.
pub fn load_multinode_crdt(doc: &Path) -> Result<Option<Vec<u8>>> {
    with_crdt_lock(doc, || {
        let path = agent_doc_fs::multinode_crdt_path_for(doc)?;
        read_crdt_state_file(&path, "per-node CRDT state")
    })
}

/// Save per-node CRDT container bytes for a document.
pub fn save_multinode_crdt(doc: &Path, state: &[u8]) -> Result<()> {
    with_crdt_lock(doc, || {
        let path = agent_doc_fs::multinode_crdt_path_for(doc)?;
        write_crdt_state_file(&path, state)
    })
}

/// Delete all CRDT state sidecars for a document. Idempotent.
pub fn delete_crdt(doc: &Path) -> Result<()> {
    let path = agent_doc_fs::crdt_path_for(doc)?;
    let overlay_path = agent_doc_fs::overlay_crdt_path_for(doc)?;
    let nodes_path = agent_doc_fs::multinode_crdt_path_for(doc)?;
    if path.exists() || overlay_path.exists() || nodes_path.exists() {
        with_crdt_lock(doc, || {
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            if overlay_path.exists() {
                std::fs::remove_file(&overlay_path)?;
            }
            if nodes_path.exists() {
                std::fs::remove_file(&nodes_path)?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Read CRDT state bytes from an already-resolved sidecar path.
pub fn read_crdt_state_file(path: &Path, label: &str) -> Result<Option<Vec<u8>>> {
    agent_doc_fs::read_optional_bytes(path)
        .with_context(|| format!("failed to read {label} {}", path.display()))
}

/// Atomically write CRDT state bytes to an already-resolved sidecar path.
pub fn write_crdt_state_file(path: &Path, state: &[u8]) -> Result<()> {
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

/// Encode markdown and write it to an already-resolved overlay CRDT sidecar.
pub fn write_overlay_crdt_state_file_from_markdown(path: &Path, markdown: &str) -> Result<usize> {
    let overlay_state = overlay_state_from_markdown(markdown);
    write_crdt_state_file(path, &overlay_state)?;
    Ok(overlay_state.len())
}

fn load_unlocked(doc: &Path) -> Result<Option<String>> {
    let snap = agent_doc_fs::snapshot_path_for(doc)?;
    if snap.exists() {
        Ok(Some(std::fs::read_to_string(&snap)?))
    } else {
        Ok(None)
    }
}

fn save_unlocked(doc: &Path, content: &str) -> Result<()> {
    let snap = agent_doc_fs::snapshot_path_for(doc)?;
    if let Some(parent) = snap.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let redacted = agent_doc_secret_redact::redact(content);
    let parent = snap.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, redacted.as_bytes())
        .with_context(|| "failed to write snapshot temp file")?;
    tmp.persist(&snap)
        .with_context(|| format!("failed to rename temp file to {}", snap.display()))?;
    Ok(())
}

fn acquire_crdt_lock(doc: &Path) -> Result<File> {
    let lock_path = agent_doc_fs::crdt_flock_path_for(doc)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
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

/// Whether the model-projected-baseline cutover is enabled.
///
/// Default is on. Opt out with `AGENT_DOC_MPS` set to `0`, `false`, `no`, or
/// `off`.
pub fn mps_enabled() -> bool {
    match std::env::var("AGENT_DOC_MPS") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Shadow-probe model projection equivalence for a snapshot save.
///
/// The probe is off unless `AGENT_DOC_MPS_PROJECTION_PROBE` is set and reports
/// through the injected logger. It never fails the caller.
pub fn probe_overlay_projection(doc: &Path, content: &str, mut logger: impl FnMut(&Path, &str)) {
    if std::env::var_os("AGENT_DOC_MPS_PROJECTION_PROBE").is_none() {
        return;
    }
    match project_overlay_roundtrip(content) {
        Ok(projected) if projected == content => {
            logger(
                doc,
                &format!(
                    "mps_projection_equiv ok len={} file={}",
                    content.len(),
                    doc.display()
                ),
            );
        }
        Ok(projected) => {
            logger(
                doc,
                &format!(
                    "mps_projection_equiv drift kind=mismatch len={} projected_len={} file={}",
                    content.len(),
                    projected.len(),
                    doc.display()
                ),
            );
        }
        Err(err) => {
            eprintln!("[mps] overlay projection failed: {err:#}");
            logger(
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

/// Persist `content` as this cycle's model-projected baseline overlay.
pub fn save_baseline_model(
    doc: &Path,
    content: &str,
    mut logger: impl FnMut(&Path, &str),
) -> Result<()> {
    let path = agent_doc_fs::baseline_overlay_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = overlay_state_from_markdown(content);
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    std::io::Write::write_all(&mut tmp, &state)
        .with_context(|| "failed to write baseline overlay temp file")?;
    tmp.persist(&path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    logger(
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

/// Project a model baseline overlay back to markdown and choose the merge base.
pub fn load_baseline_model(
    doc: &Path,
    md_baseline: Option<&str>,
    mut logger: impl FnMut(&Path, &str),
) -> Result<Option<String>> {
    let path = agent_doc_fs::baseline_overlay_path_for(doc)?;
    let bytes = match agent_doc_fs::read_optional_bytes(&path)
        .with_context(|| format!("failed to read baseline overlay {}", path.display()))?
    {
        Some(b) => b,
        None => return Ok(None),
    };
    let projection = project_overlay_state(&bytes, md_baseline)
        .with_context(|| format!("failed to project baseline overlay {}", path.display()))?;
    let resolution = resolve_model_baseline_projection(projection, md_baseline);

    match resolution.source {
        ModelBaselineSource::MdBackstop => {
            logger(
                doc,
                &format!(
                    "mps_baseline_resolve source=md_backstop file={} projected_len={} md_len={} diverged=true",
                    doc.display(),
                    resolution.projected_len,
                    resolution.md_len,
                ),
            );
            logger(
                doc,
                &format!(
                    "mps_baseline_divergence file={} projected_len={} md_len={} first_diff_byte={:?}",
                    doc.display(),
                    resolution.projected_len,
                    resolution.md_len,
                    resolution.first_diff_byte
                ),
            );
            Ok(Some(resolution.content))
        }
        ModelBaselineSource::Model => {
            logger(
                doc,
                &format!(
                    "mps_baseline_resolve source=model file={} projected_len={} md_len={} diverged=false",
                    doc.display(),
                    resolution.projected_len,
                    resolution.md_len,
                ),
            );
            Ok(Some(resolution.content))
        }
    }
}

/// Delete the model baseline sidecar for a document, if present. Idempotent.
pub fn delete_baseline_model(doc: &Path) -> Result<()> {
    let path = agent_doc_fs::baseline_overlay_path_for(doc)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn setup() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "---\nagent_doc_session: test\n---\n\nbody\n").unwrap();
        (dir, doc)
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_git(root: &Path) {
        git(root, &["init"]);
        git(root, &["config", "user.email", "test@test.com"]);
        git(root, &["config", "user.name", "Test"]);
    }

    #[test]
    fn markdown_snapshot_save_load_and_delete_roundtrips() {
        let (_dir, doc) = setup();
        let mut logs = Vec::new();

        assert!(load(&doc).unwrap().is_none());
        save(&doc, "snapshot body", |_, message| {
            logs.push(message.to_string())
        })
        .unwrap();
        assert_eq!(load(&doc).unwrap().as_deref(), Some("snapshot body"));
        assert!(logs.iter().any(|message| message.contains("snapshot_save")));

        delete(&doc).unwrap();
        assert!(load(&doc).unwrap().is_none());
        delete(&doc).unwrap();
    }

    #[test]
    fn verify_snapshot_committed_returns_committed_when_matching() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_git(root);

        let doc = root.join("doc.md");
        let content = "# Hello\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        save(&doc, content, |_, _| {}).unwrap();
        git(root, &["add", "doc.md"]);
        git(root, &["commit", "-m", "add doc", "--no-verify"]);

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::Committed,
        );
    }

    #[test]
    fn verify_snapshot_committed_returns_differs_when_snapshot_ahead() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_git(root);

        let doc = root.join("doc.md");
        let old_content = "# Hello\n\nold body\n";
        std::fs::write(&doc, old_content).unwrap();
        git(root, &["add", "doc.md"]);
        git(root, &["commit", "-m", "add doc", "--no-verify"]);

        let new_content = "# Hello\n\nnew response body\n";
        save(&doc, new_content, |_, _| {}).unwrap();

        match verify_snapshot_committed(&doc).unwrap() {
            SnapshotCommitStatus::SnapshotDiffersFromHead { .. } => {}
            other => panic!("expected SnapshotDiffersFromHead, got {:?}", other),
        }
    }

    #[test]
    fn verify_snapshot_committed_no_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_git(root);

        let doc = root.join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        git(root, &["add", "doc.md"]);
        git(root, &["commit", "-m", "add doc", "--no-verify"]);

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::NoSnapshot,
        );
    }

    #[test]
    fn verify_snapshot_committed_no_head() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_git(root);

        let doc = root.join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        save(&doc, "body\n", |_, _| {}).unwrap();

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::NoHead,
        );
    }

    #[test]
    fn verify_snapshot_committed_outside_git_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        save(&doc, "body\n", |_, _| {}).unwrap();

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::NotInGitRepo,
        );
    }

    #[test]
    fn markdown_snapshot_lock_file_is_removed_on_drop() {
        let (_dir, doc) = setup();

        let lock = SnapshotLock::acquire(&doc).unwrap();
        let lock_path = agent_doc_fs::snapshot_flock_path_for(&doc).unwrap();
        assert!(lock_path.exists());
        drop(lock);

        assert!(!lock_path.exists());
    }

    #[test]
    fn pre_response_snapshot_save_load_and_delete_roundtrips() {
        let (_dir, doc) = setup();

        assert!(load_pre_response(&doc).unwrap().is_none());
        save_pre_response(&doc, "pre response").unwrap();
        assert_eq!(
            load_pre_response(&doc).unwrap().as_deref(),
            Some("pre response")
        );
        delete_pre_response(&doc).unwrap();
        assert!(load_pre_response(&doc).unwrap().is_none());
        delete_pre_response(&doc).unwrap();
    }

    #[test]
    fn ensure_initial_snapshot_creates_once_with_projection() {
        let (_dir, doc) = setup();
        std::fs::write(&doc, "before\n<!-- strip -->\nafter\n").unwrap();
        let mut logs = Vec::new();

        let created = ensure_initial_snapshot(
            &doc,
            |content| content.replace("<!-- strip -->\n", ""),
            |_, message| logs.push(message.to_string()),
        )
        .unwrap();
        let created_again =
            ensure_initial_snapshot(&doc, |content| content.to_string(), |_, _| {}).unwrap();

        assert!(created);
        assert!(!created_again);
        assert_eq!(load(&doc).unwrap().as_deref(), Some("before\nafter\n"));
        assert!(logs.iter().any(|message| message.contains("snapshot_save")));
    }

    #[test]
    fn migrate_state_files_for_hash_moves_known_sidecars_and_reports_events() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let old_hash = "oldhash123456";
        let new_hash = "newhashabcdef";
        for (subdir, ext, bytes) in [
            ("snapshots", "md", b"snapshot".as_slice()),
            ("baselines", "overlay.yrs", b"overlay"),
            ("crdt", "nodes.yrs", b"nodes"),
            ("pre-response", "md", b"pre"),
        ] {
            let state_dir = root.join(".agent-doc").join(subdir);
            std::fs::create_dir_all(&state_dir).unwrap();
            std::fs::write(state_dir.join(format!("{old_hash}.{ext}")), bytes).unwrap();
        }
        std::fs::create_dir_all(root.join(".agent-doc/locks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/locks")
                .join(format!("{old_hash}.md.lock")),
            b"lock",
        )
        .unwrap();

        let report = migrate_state_files_for_hash(root, old_hash, new_hash).unwrap();

        assert_eq!(report.migrated, 5);
        assert!(root.join(".agent-doc/snapshots/newhashabcdef.md").exists());
        assert!(
            root.join(".agent-doc/baselines/newhashabcdef.overlay.yrs")
                .exists()
        );
        assert!(
            root.join(".agent-doc/crdt/newhashabcdef.nodes.yrs")
                .exists()
        );
        assert!(
            root.join(".agent-doc/pre-response/newhashabcdef.md")
                .exists()
        );
        assert!(root.join(".agent-doc/locks/newhashabcdef.md.lock").exists());
        assert_eq!(report.events.len(), 4);
        assert_eq!(
            report.events[0].log_message(),
            "[init] migrated snapshots/oldhash1.md → newhasha.md"
        );
    }

    #[test]
    fn migrate_state_files_for_hash_skips_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let old_hash = "oldhash123456";
        let new_hash = "newhashabcdef";
        let state_dir = root.join(".agent-doc/snapshots");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join(format!("{old_hash}.md")), b"old").unwrap();
        std::fs::write(state_dir.join(format!("{new_hash}.md")), b"new").unwrap();

        let report = migrate_state_files_for_hash(root, old_hash, new_hash).unwrap();

        assert_eq!(report.migrated, 0);
        assert_eq!(
            std::fs::read(state_dir.join(format!("{old_hash}.md"))).unwrap(),
            b"old"
        );
        assert_eq!(
            report.events,
            vec![SnapshotStateMigrationEvent::SkippedDestinationExists {
                subdir: "snapshots".to_string(),
                new_hash_prefix: "newhasha".to_string(),
                ext: "md".to_string(),
            }]
        );
    }

    #[test]
    fn find_snapshot_hash_for_session_skips_current_and_matches_session() {
        let dir = tempfile::tempdir().unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        std::fs::create_dir_all(&snap_dir).unwrap();
        std::fs::write(
            snap_dir.join("current.md"),
            "---\nagent_doc_session: target\n---\ncurrent\n",
        )
        .unwrap();
        std::fs::write(
            snap_dir.join("oldhash.md"),
            "---\nagent_doc_session: target\n---\nold\n",
        )
        .unwrap();
        std::fs::write(
            snap_dir.join("other.txt"),
            "---\nagent_doc_session: target\n---\nignored\n",
        )
        .unwrap();

        let found = find_snapshot_hash_for_session(&snap_dir, "current", "target").unwrap();

        assert_eq!(found.as_deref(), Some("oldhash"));
    }

    #[test]
    fn find_snapshot_hash_for_session_returns_none_without_match() {
        let dir = tempfile::tempdir().unwrap();
        let snap_dir = dir.path().join(".agent-doc/snapshots");
        std::fs::create_dir_all(&snap_dir).unwrap();
        std::fs::write(
            snap_dir.join("oldhash.md"),
            "---\nagent_doc_session: other\n---\nold\n",
        )
        .unwrap();

        let found = find_snapshot_hash_for_session(&snap_dir, "current", "target").unwrap();
        let missing_dir =
            find_snapshot_hash_for_session(&snap_dir.join("missing"), "current", "target").unwrap();

        assert!(found.is_none());
        assert!(missing_dir.is_none());
    }

    #[test]
    fn crdt_state_save_load_and_delete_roundtrips_all_sidecars() {
        let (_dir, doc) = setup();

        assert!(load_crdt(&doc).unwrap().is_none());
        assert!(load_overlay_crdt(&doc).unwrap().is_none());
        assert!(load_multinode_crdt(&doc).unwrap().is_none());

        save_crdt(&doc, b"legacy").unwrap();
        save_overlay_crdt(&doc, b"overlay").unwrap();
        save_multinode_crdt(&doc, b"nodes").unwrap();

        assert_eq!(load_crdt(&doc).unwrap().as_deref(), Some(&b"legacy"[..]));
        assert_eq!(
            load_overlay_crdt(&doc).unwrap().as_deref(),
            Some(&b"overlay"[..])
        );
        assert_eq!(
            load_multinode_crdt(&doc).unwrap().as_deref(),
            Some(&b"nodes"[..])
        );

        delete_crdt(&doc).unwrap();
        assert!(load_crdt(&doc).unwrap().is_none());
        assert!(load_overlay_crdt(&doc).unwrap().is_none());
        assert!(load_multinode_crdt(&doc).unwrap().is_none());
        delete_crdt(&doc).unwrap();
    }

    #[test]
    fn overlay_crdt_from_markdown_writes_projectable_state() {
        let (_dir, doc) = setup();
        let markdown = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n"
        );

        let overlay_bytes = save_overlay_crdt_from_markdown(&doc, markdown).unwrap();
        let state = load_overlay_crdt(&doc).unwrap().unwrap();
        assert_eq!(state.len(), overlay_bytes);
        let projected =
            agent_doc_document::model_projection::project_overlay_state(&state, Some(markdown))
                .unwrap();
        assert_eq!(projected, markdown);
    }

    #[test]
    fn baseline_model_roundtrips_byte_identical() {
        let (_dir, doc) = setup();
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n- do [#a]\n- do [#b]\n<!-- /agent:queue -->\n"
        );

        save_baseline_model(&doc, baseline, |_, _| {}).unwrap();
        let projected = load_baseline_model(&doc, Some(baseline), |_, _| {}).unwrap();

        assert_eq!(projected.as_deref(), Some(baseline));
    }

    #[test]
    fn baseline_model_none_when_absent() {
        let (_dir, doc) = setup();

        assert!(
            load_baseline_model(&doc, Some("anything"), |_, _| {})
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn baseline_model_prefers_md_backstop_on_divergence_and_logs() {
        let (_dir, doc) = setup();
        let pinned = "## Queue\n<!-- agent:queue -->\n- do [#real]\n<!-- /agent:queue -->\n";
        let stale_md = "## Queue\n<!-- agent:queue -->\n- do [#stale]\n<!-- /agent:queue -->\n";
        let mut logs = Vec::new();

        save_baseline_model(&doc, pinned, |_, _| {}).unwrap();
        let resolved = load_baseline_model(&doc, Some(stale_md), |_, message| {
            logs.push(message.to_string())
        })
        .unwrap();

        assert_eq!(resolved.as_deref(), Some(stale_md));
        assert!(
            logs.iter()
                .any(|message| { message.contains("mps_baseline_resolve source=md_backstop") })
        );
        assert!(
            logs.iter()
                .any(|message| { message.contains("mps_baseline_divergence") })
        );
    }

    #[test]
    fn baseline_model_uses_projection_when_no_md() {
        let (_dir, doc) = setup();
        let pinned = "## Queue\n<!-- agent:queue -->\n- do [#only]\n<!-- /agent:queue -->\n";

        save_baseline_model(&doc, pinned, |_, _| {}).unwrap();
        let resolved = load_baseline_model(&doc, None, |_, _| {}).unwrap();

        assert_eq!(resolved.as_deref(), Some(pinned));
    }

    #[test]
    fn delete_baseline_model_is_idempotent() {
        let (_dir, doc) = setup();

        delete_baseline_model(&doc).unwrap();
        save_baseline_model(&doc, "x\n", |_, _| {}).unwrap();
        assert!(
            agent_doc_fs::baseline_overlay_path_for(&doc)
                .unwrap()
                .exists()
        );

        delete_baseline_model(&doc).unwrap();
        assert!(
            !agent_doc_fs::baseline_overlay_path_for(&doc)
                .unwrap()
                .exists()
        );
        delete_baseline_model(&doc).unwrap();
    }
}

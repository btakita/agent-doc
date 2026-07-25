//! Durable document-baseline authority plus cold snapshot recovery projections.

use anyhow::{Context, Result};
use base64::Engine as _;
use std::path::Path;

use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;
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

/// Load the content-bearing merge baseline from the durable state ledger.
pub fn load_document_baseline(doc: &Path) -> Result<Option<String>> {
    Ok(load_document_state_projection(doc)?
        .and_then(|projection| projection.document.merge_baseline)
        .map(|baseline| baseline.content))
}

/// Checkpoint the content-bearing merge baseline in the durable state ledger.
pub fn checkpoint_document_baseline(
    doc: &Path,
    content: &str,
    mut logger: impl FnMut(&Path, &str),
) -> Result<()> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let document_hash = agent_doc_hash::content_hash(&canonical.display().to_string());
    let generation = load_document_state_projection(doc)?
        .map(|projection| projection.document.merge_baseline_generation)
        .unwrap_or(0)
        .saturating_add(1);
    let content_hash = agent_doc_hash::content_hash(content);
    let fact = agent_doc_state_backbone::StateFact::DocumentBaselineCheckpointed {
        document_hash: document_hash.clone(),
        generation,
        content_hash: content_hash.clone(),
        content: content.to_string(),
    };
    append_document_fact(
        doc,
        format!("document-baseline:{document_hash}:{generation}:{content_hash}"),
        fact,
    )?;
    logger(
        doc,
        &format!(
            "document_baseline_checkpoint file={} generation={} len={}",
            doc.display(),
            generation,
            content.len()
        ),
    );
    Ok(())
}

fn load_document_state_projection(
    doc: &Path,
) -> Result<Option<agent_doc_state_backbone::DocumentStateProjection>> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    if !agent_doc_sqlite::state_store::state_db_path(&project_root).exists() {
        return Ok(None);
    }
    let document_hash = agent_doc_hash::content_hash(&canonical.display().to_string());
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let events = agent_doc_sqlite::state_store::load_state_events_for_cycle_projection_from_db(
        &conn,
        &document_hash,
    )?;
    let mut projection = agent_doc_state_backbone::DocumentStateProjection::new(&document_hash);
    for status in events {
        let event: agent_doc_state_backbone::StateEvent =
            serde_json::from_str(&status.payload_json)
                .with_context(|| format!("parse state event {}", status.event_id))?;
        projection.apply_fact(&event.fact);
    }
    Ok(Some(projection))
}

fn append_document_fact(
    doc: &Path,
    event_id: String,
    fact: agent_doc_state_backbone::StateFact,
) -> Result<()> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    let event = agent_doc_state_backbone::StateEvent::new(event_id, fact);
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let payload_json = serde_json::to_string(&event).context("serialize document state event")?;
    agent_doc_sqlite::state_store::insert_state_event_in_db(
        &conn,
        &agent_doc_sqlite::state_store::StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    )?;
    Ok(())
}

/// Clear the normal merge baseline while retaining an auditable ledger fact.
pub fn clear_document_baseline(doc: &Path) -> Result<()> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let document_hash = agent_doc_hash::content_hash(&canonical.display().to_string());
    let generation = load_document_state_projection(doc)?
        .map(|projection| projection.document.merge_baseline_generation)
        .unwrap_or(0)
        .saturating_add(1);
    append_document_fact(
        doc,
        format!("document-baseline-clear:{document_hash}:{generation}"),
        agent_doc_state_backbone::StateFact::DocumentBaselineCleared {
            document_hash,
            generation,
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrdtRecoveryProjection {
    pub projection: Vec<u8>,
    pub lineage: String,
}

/// Load the cold CRDT restart projection from the durable state ledger.
pub fn load_crdt_recovery_projection(doc: &Path) -> Result<Option<CrdtRecoveryProjection>> {
    let Some(projected) = load_document_state_projection(doc)?
        .and_then(|projection| projection.document.crdt_recovery_projection)
    else {
        return Ok(None);
    };
    let projection = base64::engine::general_purpose::STANDARD
        .decode(&projected.projection_base64)
        .context("decode CRDT recovery projection")?;
    anyhow::ensure!(
        agent_doc_hash::bytes_hash(&projection) == projected.projection_sha256,
        "CRDT recovery projection hash mismatch"
    );
    Ok(Some(CrdtRecoveryProjection {
        projection,
        lineage: projected.lineage,
    }))
}

/// Checkpoint a cold CRDT restart projection without creating a sidecar file.
pub fn checkpoint_crdt_recovery_projection(
    doc: &Path,
    projection: &[u8],
    lineage: &str,
) -> Result<bool> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let document_hash = agent_doc_hash::content_hash(&canonical.display().to_string());
    let document_projection = load_document_state_projection(doc)?
        .map(|projection| projection.document)
        .unwrap_or_default();
    let prior_generation = document_projection.crdt_recovery_projection_generation;
    let prior = document_projection.crdt_recovery_projection;
    let projection_sha256 = agent_doc_hash::bytes_hash(projection);
    if prior.as_ref().is_some_and(|prior| {
        prior.projection_sha256 == projection_sha256 && prior.lineage == lineage
    }) {
        return Ok(false);
    }
    let generation = prior_generation.saturating_add(1);
    append_document_fact(
        doc,
        format!("crdt-recovery:{document_hash}:{generation}:{projection_sha256}"),
        agent_doc_state_backbone::StateFact::CrdtRecoveryProjectionCheckpointed {
            document_hash,
            generation,
            projection_sha256,
            projection_base64: base64::engine::general_purpose::STANDARD.encode(projection),
            lineage: lineage.to_string(),
        },
    )?;
    Ok(true)
}

/// Clear the cold CRDT restart projection without deleting ledger history.
pub fn clear_crdt_recovery_projection(doc: &Path) -> Result<()> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let document_hash = agent_doc_hash::content_hash(&canonical.display().to_string());
    let generation = load_document_state_projection(doc)?
        .map(|projection| projection.document.crdt_recovery_projection_generation)
        .unwrap_or(0)
        .saturating_add(1);
    append_document_fact(
        doc,
        format!("crdt-recovery-clear:{document_hash}:{generation}"),
        agent_doc_state_backbone::StateFact::CrdtRecoveryProjectionCleared {
            document_hash,
            generation,
        },
    )
}

/// Resolve the best markdown snapshot content for diff computation.
///
/// The snapshot file is authoritative when present. Git is only used as a
/// recovery fallback when no snapshot exists and HEAD differs from the current
/// worktree file.
pub fn resolve(doc: &Path) -> Result<Option<String>> {
    if let Some(baseline) = load_document_baseline(doc)? {
        return Ok(Some(baseline));
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
pub struct DiffBaselineStore {
    logger: fn(&Path, &str),
}

impl DiffBaselineStore {
    pub const fn new(logger: fn(&Path, &str)) -> Self {
        Self { logger }
    }
}

impl agent_doc_diff_io::DocumentBaselineStore for DiffBaselineStore {
    fn resolve(&self, doc: &Path) -> Result<Option<String>> {
        resolve(doc)
    }

    fn checkpoint(&self, doc: &Path, content: &str) -> Result<()> {
        checkpoint_document_baseline(doc, content, self.logger)
    }
}

/// Delete a cold markdown recovery projection and clear the ledger baseline.
pub fn delete_recovery_projection_and_clear_baseline(doc: &Path) -> Result<()> {
    let snap = agent_doc_fs::snapshot_path_for(doc)?;
    if !snap.exists() {
        return clear_document_baseline(doc);
    }
    if snap.exists() {
        std::fs::remove_file(&snap)?;
    }
    clear_document_baseline(doc)
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
    let snapshot = load_document_baseline(file)?;
    let head_doc = agent_doc_git_io::revision::show_head(file)?;
    Ok(snapshot_commit_status_from_contents(
        snapshot.as_deref(),
        head_doc.as_deref(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotHeadContentHashStatus {
    Matching {
        hash: String,
    },
    SnapshotDiffersFromHead {
        snapshot_hash: String,
        head_hash: String,
        snapshot_len: usize,
        head_len: usize,
    },
    NoSnapshot,
    NoHead,
    NotInGitRepo,
}

/// Compare the raw snapshot content hash against the raw `HEAD:<file>` hash.
///
/// This is stricter than [`verify_snapshot_committed`], which normalizes
/// transient markers for user-facing drift guards. Terminal closeout proof
/// records raw hashes, so its retry gate must use the same raw equality.
pub fn verify_snapshot_head_content_hash(file: &Path) -> Result<SnapshotHeadContentHashStatus> {
    if !agent_doc_git_io::status::is_in_git_repo(file) {
        return Ok(SnapshotHeadContentHashStatus::NotInGitRepo);
    }
    let Some(snapshot) = load_document_baseline(file)? else {
        return Ok(SnapshotHeadContentHashStatus::NoSnapshot);
    };
    let Some(head_doc) = agent_doc_git_io::revision::show_head(file)? else {
        return Ok(SnapshotHeadContentHashStatus::NoHead);
    };
    let snapshot_hash = agent_doc_hash::content_hash(&snapshot);
    let head_hash = agent_doc_hash::content_hash(&head_doc);
    if snapshot_hash == head_hash {
        Ok(SnapshotHeadContentHashStatus::Matching {
            hash: snapshot_hash,
        })
    } else {
        Ok(SnapshotHeadContentHashStatus::SnapshotDiffersFromHead {
            snapshot_hash,
            head_hash,
            snapshot_len: snapshot.len(),
            head_len: head_doc.len(),
        })
    }
}

/// Create the initial durable merge baseline if no baseline exists yet.
///
/// The caller supplies the content projection so domain-specific stripping or
/// normalization stays outside snapshot IO. This helper owns the file/path
/// existence check, document read, and ledger checkpoint.
pub fn ensure_initial_snapshot(
    doc: &Path,
    project_content: impl FnOnce(&str) -> String,
    logger: impl FnMut(&Path, &str),
) -> Result<bool> {
    if load_document_baseline(doc)?.is_some() {
        return Ok(false);
    }
    eprintln!(
        "[init] creating document baseline for {} (none found)",
        doc.display()
    );
    if let Ok(content) = std::fs::read_to_string(doc) {
        let snapshot_content = project_content(&content);
        checkpoint_document_baseline(doc, &snapshot_content, logger)?;
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
    const MIGRATE_DIRS: &[(&str, &str)] =
        &[("snapshots", "md"), ("locks", "lock"), ("pending", "md")];

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

    // Migrate the cold snapshot lock with its projection.
    let lock_dir = project_root.join(".agent-doc").join("locks");
    let old_lock_name = format!("{}.md.lock", old_hash);
    let old_lock = lock_dir.join(&old_lock_name);
    let new_lock = lock_dir.join(old_lock_name.replace(old_hash, new_hash));
    if old_lock.exists() && !new_lock.exists() {
        std::fs::rename(&old_lock, &new_lock)?;
        report.migrated += 1;
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

/// Checkpoint the pre-write content used by undo/extract in the durable ledger.
pub fn checkpoint_undo_content(doc: &Path, content: &str) -> Result<()> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let document_hash = agent_doc_hash::content_hash(&canonical.display().to_string());
    let generation = load_document_state_projection(doc)?
        .map(|projection| projection.document.undo_checkpoint_generation)
        .unwrap_or(0)
        .saturating_add(1);
    let redacted = agent_doc_secret_redact::redact(content);
    let content_hash = agent_doc_hash::content_hash(&redacted);
    append_document_fact(
        doc,
        format!("undo-checkpoint:{document_hash}:{generation}:{content_hash}"),
        agent_doc_state_backbone::StateFact::UndoCheckpointed {
            document_hash,
            generation,
            content_hash,
            content: redacted,
        },
    )
}

/// Load the active pre-write undo checkpoint from the durable ledger.
pub fn load_undo_content(doc: &Path) -> Result<Option<String>> {
    Ok(load_document_state_projection(doc)?
        .and_then(|projection| projection.document.undo_checkpoint)
        .map(|checkpoint| checkpoint.content))
}

/// Clear the active undo checkpoint without deleting durable history.
pub fn clear_undo_content(doc: &Path) -> Result<()> {
    let canonical = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let document_hash = agent_doc_hash::content_hash(&canonical.display().to_string());
    let generation = load_document_state_projection(doc)?
        .map(|projection| projection.document.undo_checkpoint_generation)
        .unwrap_or(0)
        .saturating_add(1);
    append_document_fact(
        doc,
        format!("undo-checkpoint-clear:{document_hash}:{generation}"),
        agent_doc_state_backbone::StateFact::UndoCheckpointCleared {
            document_hash,
            generation,
        },
    )
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

        assert!(load_document_baseline(&doc).unwrap().is_none());
        checkpoint_document_baseline(&doc, "snapshot body", |_, message| {
            logs.push(message.to_string())
        })
        .unwrap();
        assert_eq!(
            load_document_baseline(&doc).unwrap().as_deref(),
            Some("snapshot body")
        );
        assert!(
            logs.iter()
                .any(|message| message.contains("document_baseline_checkpoint"))
        );

        delete_recovery_projection_and_clear_baseline(&doc).unwrap();
        assert!(load_document_baseline(&doc).unwrap().is_none());
        delete_recovery_projection_and_clear_baseline(&doc).unwrap();
    }

    #[test]
    fn verify_snapshot_committed_returns_committed_when_matching() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_git(root);

        let doc = root.join("doc.md");
        let content = "# Hello\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        checkpoint_document_baseline(&doc, content, |_, _| {}).unwrap();
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
        checkpoint_document_baseline(&doc, new_content, |_, _| {}).unwrap();

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
        checkpoint_document_baseline(&doc, "body\n", |_, _| {}).unwrap();

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
        checkpoint_document_baseline(&doc, "body\n", |_, _| {}).unwrap();

        assert_eq!(
            verify_snapshot_committed(&doc).unwrap(),
            SnapshotCommitStatus::NotInGitRepo,
        );
    }

    #[test]
    fn undo_checkpoint_roundtrips_through_state_ledger() {
        let (_dir, doc) = setup();

        assert!(load_undo_content(&doc).unwrap().is_none());
        checkpoint_undo_content(&doc, "pre response").unwrap();
        assert_eq!(
            load_undo_content(&doc).unwrap().as_deref(),
            Some("pre response")
        );
        clear_undo_content(&doc).unwrap();
        assert!(load_undo_content(&doc).unwrap().is_none());
        clear_undo_content(&doc).unwrap();
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
        assert_eq!(
            load_document_baseline(&doc).unwrap().as_deref(),
            Some("before\nafter\n")
        );
        assert!(
            logs.iter()
                .any(|message| message.contains("document_baseline_checkpoint"))
        );
    }

    #[test]
    fn migrate_state_files_for_hash_moves_cold_snapshot_and_lock() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let old_hash = "oldhash123456";
        let new_hash = "newhashabcdef";
        for (subdir, ext, bytes) in [("snapshots", "md", b"snapshot".as_slice())] {
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

        assert_eq!(report.migrated, 2);
        assert!(root.join(".agent-doc/snapshots/newhashabcdef.md").exists());
        assert!(root.join(".agent-doc/locks/newhashabcdef.md.lock").exists());
        assert_eq!(report.events.len(), 1);
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
}

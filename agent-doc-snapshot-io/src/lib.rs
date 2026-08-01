//! Durable document-baseline authority plus write-only crash-state sidecars.

use anyhow::{Context, Result};
use base64::Engine as _;
use std::path::{Path, PathBuf};

use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;
use agent_doc_frontmatter::frontmatter::session_id_from_content;
use agent_doc_git_io::revision::HeadWorktreeFallback;

/// Downstream filesystem effects projected from authoritative typed state.
///
/// Implementations may write or remove crash state, but they are never a
/// read authority for document state.
pub trait CrashStateEffects {
    fn write_markdown_crash_state(&self, doc: &Path, content: &str) -> Result<()>;

    fn clear_markdown_crash_state(&self, doc: &Path) -> Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FilesystemCrashStateEffects;

impl CrashStateEffects for FilesystemCrashStateEffects {
    fn write_markdown_crash_state(&self, doc: &Path, content: &str) -> Result<()> {
        let path = agent_doc_fs::snapshot_path_for(doc)?;
        agent_doc_fs::write_atomic(&path, content.as_bytes())
            .with_context(|| format!("write crash-state sidecar {}", path.display()))
    }

    fn clear_markdown_crash_state(&self, doc: &Path) -> Result<()> {
        let path = agent_doc_fs::snapshot_path_for(doc)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("remove crash-state sidecar {}", path.display()))
            }
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
    logger: impl FnMut(&Path, &str),
) -> Result<()> {
    checkpoint_document_baseline_with_effects(doc, content, logger, &FilesystemCrashStateEffects)
}

/// Append the typed baseline fact, then write the crash-state sidecar effect.
///
/// The ordering is intentional: the sidecar is an effect of committed typed
/// state and can never become the input that creates or repairs that state.
pub fn checkpoint_document_baseline_with_effects(
    doc: &Path,
    content: &str,
    mut logger: impl FnMut(&Path, &str),
    effects: &dyn CrashStateEffects,
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
    effects.write_markdown_crash_state(doc, content)?;
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
    clear_document_baseline_with_effects(doc, &FilesystemCrashStateEffects)
}

/// Append the typed clear fact, then remove its write-only crash-state sidecar.
pub fn clear_document_baseline_with_effects(
    doc: &Path,
    effects: &dyn CrashStateEffects,
) -> Result<()> {
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
    )?;
    effects.clear_markdown_crash_state(doc)
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

/// Resolve the best durable baseline content for diff computation.
///
/// Typed ledger state is authoritative. Git is only used as a first-submit
/// fallback when no ledger baseline exists and HEAD differs from the current
/// worktree file. Crash sidecars are never read.
pub fn resolve(doc: &Path) -> Result<Option<String>> {
    if let Some(baseline) = load_document_baseline(doc)? {
        return Ok(Some(baseline));
    }

    match agent_doc_git_io::revision::head_fallback_when_differs_from_worktree(doc)? {
        HeadWorktreeFallback::NoHead => Ok(None),
        HeadWorktreeFallback::MatchesCurrent => {
            eprintln!(
                "[snapshot] No ledger baseline, git matches current — treating as first submit"
            );
            Ok(None)
        }
        HeadWorktreeFallback::DiffersFromCurrent(git_content) => {
            eprintln!("[snapshot] No ledger baseline, recovering first-submit baseline from git");
            Ok(Some(git_content))
        }
    }
}

/// Diff-IO baseline adapter backed by typed ledger state.
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
    let Ok(content) = std::fs::read_to_string(doc) else {
        return Ok(false);
    };
    ensure_initial_snapshot_with_content(doc, &content, project_content, logger)
}

/// Create the initial baseline from an already-resolved authority projection.
///
/// Editor-owned documents may contain unsaved text that is newer than disk.
/// Callers that already crossed that authority boundary must pass the resolved
/// projection through instead of reopening the file.
pub fn ensure_initial_snapshot_with_content(
    doc: &Path,
    content: &str,
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
    let snapshot_content = project_content(content);
    checkpoint_document_baseline(doc, &snapshot_content, logger)?;
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedDocumentPathTransition {
    pub project_root: PathBuf,
    pub session_id: String,
    pub old_path: PathBuf,
    pub new_path: PathBuf,
}

/// Detect a document rename from the durable session registry.
///
/// A document rename changes the path-derived state hash. The session registry
/// supplies the previous path identity. This adapter is observation-only:
/// callers send the returned transition through the Project Controller, which
/// owns the durable state migration. Crash sidecars are never scanned, parsed,
/// moved, or used as fallback state.
pub fn detect_document_path_transition(
    doc: &Path,
) -> Result<Option<DetectedDocumentPathTransition>> {
    let session_uuid = match std::fs::read_to_string(doc)
        .ok()
        .and_then(|content| session_id_from_content(&content))
    {
        Some(uuid) => uuid,
        None => return Ok(None),
    };

    let canonical = doc.canonicalize()?;
    let project_root = match agent_doc_project_root_io::project_root_containing(&canonical) {
        Some(root) => root,
        None => return Ok(None),
    };
    let Some(previous_entry) =
        agent_doc_session_registry_io::lookup_entry_in(&project_root, &session_uuid)?
    else {
        return Ok(None);
    };
    let previous_path = Path::new(&previous_entry.file);
    let previous_path = if previous_path.is_absolute() {
        previous_path.to_path_buf()
    } else {
        project_root.join(previous_path)
    };
    if previous_path == canonical {
        return Ok(None);
    }

    let new_hash = agent_doc_fs::document_state_hash(doc)?;
    let old_hash = if previous_path.exists() {
        agent_doc_fs::document_state_hash(&previous_path)?
    } else {
        agent_doc_fs::document_state_hash_from_str(&previous_path.to_string_lossy())
    };
    if old_hash == new_hash {
        return Ok(None);
    }

    Ok(Some(DetectedDocumentPathTransition {
        project_root,
        session_id: session_uuid,
        old_path: previous_path,
        new_path: canonical,
    }))
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
    use std::cell::Cell;
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
        let crash_projection = agent_doc_fs::snapshot_path_for(&doc).unwrap();
        assert_eq!(
            std::fs::read_to_string(&crash_projection).unwrap(),
            "snapshot body",
            "the typed checkpoint should drive the write-only crash-state sidecar"
        );
        assert!(
            logs.iter()
                .any(|message| message.contains("document_baseline_checkpoint"))
        );

        delete_recovery_projection_and_clear_baseline(&doc).unwrap();
        assert!(load_document_baseline(&doc).unwrap().is_none());
        assert!(!crash_projection.exists());
        delete_recovery_projection_and_clear_baseline(&doc).unwrap();
    }

    struct StateFirstCrashEffect {
        saw_committed_typed_state: Cell<bool>,
    }

    impl CrashStateEffects for StateFirstCrashEffect {
        fn write_markdown_crash_state(&self, doc: &Path, content: &str) -> Result<()> {
            self.saw_committed_typed_state
                .set(load_document_baseline(doc)?.as_deref() == Some(content));
            Ok(())
        }

        fn clear_markdown_crash_state(&self, doc: &Path) -> Result<()> {
            self.saw_committed_typed_state
                .set(load_document_baseline(doc)?.is_none());
            Ok(())
        }
    }

    #[test]
    fn crash_projection_effect_runs_after_typed_state_commit() {
        let (_dir, doc) = setup();
        let effect = StateFirstCrashEffect {
            saw_committed_typed_state: Cell::new(false),
        };

        checkpoint_document_baseline_with_effects(&doc, "state first", |_, _| {}, &effect).unwrap();

        assert!(
            effect.saw_committed_typed_state.get(),
            "crash-state effect must observe the authoritative typed checkpoint"
        );
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
    fn renamed_document_is_detected_from_registry_without_reading_crash_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let old_doc = root.join("old.md");
        let new_doc = root.join("new.md");
        let session_id = "rename-session";
        let document = format!(
            "---\nagent_doc_session: {session_id}\nagent_doc_format: template\n---\n\nBody\n"
        );
        std::fs::write(&old_doc, &document).unwrap();
        checkpoint_document_baseline(&old_doc, "ledger baseline", |_, _| {}).unwrap();
        let old_hash = agent_doc_fs::document_state_hash(&old_doc).unwrap();
        let old_crash_sidecar = root
            .join(".agent-doc/snapshots")
            .join(format!("{old_hash}.md"));
        std::fs::write(&old_crash_sidecar, "not a valid session document").unwrap();
        agent_doc_session_registry_io::registration::attach_projection_only_in(
            root,
            session_id,
            "%1",
            &old_doc.display().to_string(),
            123,
            "@1",
            &root.display().to_string(),
        )
        .unwrap();
        std::fs::rename(&old_doc, &new_doc).unwrap();

        let transition = detect_document_path_transition(&new_doc)
            .unwrap()
            .expect("registry path drift must produce a controller observation");
        assert_eq!(transition.project_root, root);
        assert_eq!(transition.session_id, session_id);
        assert_eq!(transition.old_path, old_doc);
        assert_eq!(transition.new_path, new_doc);
        assert!(
            old_crash_sidecar.exists(),
            "write-only crash-state sidecar must remain untouched"
        );
        let new_hash = agent_doc_fs::document_state_hash(&new_doc).unwrap();
        assert!(
            !root
                .join(".agent-doc/snapshots")
                .join(format!("{new_hash}.md"))
                .exists(),
            "rename recovery must not create or migrate a crash sidecar"
        );
        let entry = agent_doc_session_registry_io::lookup_entry_in(root, session_id)
            .unwrap()
            .expect("observation does not mutate the registry");
        assert_eq!(entry.file, old_doc.display().to_string());
    }
}

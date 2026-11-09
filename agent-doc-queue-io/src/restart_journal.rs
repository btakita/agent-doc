//! Durable, disk-bounded restart journal (`#restartjournal`).
//!
//! This is a cold-start hydration/sink adapter. Queue claims, queue lineage,
//! and supervisor lineage remain authoritative for live transitions. The
//! journal stores only compact typed checkpoints and hashes, then projects a
//! recovery action after process or OS restart.

use std::path::{Path, PathBuf};

use agent_doc_queue::queue_claim::{QueueClaimLedger, QueueClaimState};
use agent_doc_queue::restart_recovery::{
    ArtifactReference, MainAgentCheckpoint, MainAgentPhase, RestartJournal, RestartJournalLimits,
    RestartRecoveryProjection, WorkerCheckpoint, WorkerPhase,
};
use anyhow::{Context, Result, bail};

pub const RESTART_JOURNAL_STATE_KIND: &str = "restart_journal_v1";

const ENV_MAX_TRANSITIONS: &str = "AGENT_DOC_RESTART_JOURNAL_MAX_TRANSITIONS";
const ENV_RECENT_GENERATIONS: &str = "AGENT_DOC_RESTART_JOURNAL_RECENT_GENERATIONS";
const ENV_COMPACT_EVERY: &str = "AGENT_DOC_RESTART_JOURNAL_COMPACT_EVERY";
const ENV_TERMINAL_RETENTION_SECS: &str = "AGENT_DOC_RESTART_JOURNAL_TERMINAL_RETENTION_SECS";
const ENV_DOCUMENT_BYTE_CAP: &str = "AGENT_DOC_RESTART_JOURNAL_DOCUMENT_BYTE_CAP";
const ENV_GLOBAL_BYTE_CAP: &str = "AGENT_DOC_RESTART_JOURNAL_GLOBAL_BYTE_CAP";

pub fn configured_limits() -> RestartJournalLimits {
    let defaults = RestartJournalLimits::default();
    RestartJournalLimits {
        max_transitions: env_usize(ENV_MAX_TRANSITIONS).unwrap_or(defaults.max_transitions),
        recent_generations: env_usize(ENV_RECENT_GENERATIONS)
            .unwrap_or(defaults.recent_generations),
        compact_every: env_usize(ENV_COMPACT_EVERY).unwrap_or(defaults.compact_every),
        terminal_retention_secs: env_u64(ENV_TERMINAL_RETENTION_SECS)
            .unwrap_or(defaults.terminal_retention_secs),
        document_byte_cap: env_usize(ENV_DOCUMENT_BYTE_CAP).unwrap_or(defaults.document_byte_cap),
        global_byte_cap: env_usize(ENV_GLOBAL_BYTE_CAP).unwrap_or(defaults.global_byte_cap),
    }
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok()?.trim().parse().ok()
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

pub fn current_boot_id() -> String {
    if let Ok(value) = std::env::var("AGENT_DOC_TEST_BOOT_ID")
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    #[cfg(target_os = "linux")]
    if let Ok(value) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        && !value.trim().is_empty()
    {
        return value.trim().to_string();
    }
    // Non-Linux hosts cannot prove an OS boundary. The stable fallback makes
    // recovery fail closed as same-boot reattachment instead of guessing that
    // a worker died.
    "boot-id-unavailable".to_string()
}

fn state_identity(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let document_hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize restart-journal document: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((
        root,
        document_hash,
        canonical.to_string_lossy().into_owned(),
    )))
}

pub fn load(file: &Path) -> Result<RestartJournal> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(RestartJournal::default());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from_conn(&conn, &document_hash)
}

fn load_from_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<RestartJournal> {
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        RESTART_JOURNAL_STATE_KIND,
    )?
    else {
        return Ok(RestartJournal::default());
    };
    serde_json::from_str(&record.payload_json).context("decode restart journal")
}

fn store_with_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
    canonical_path: &str,
    journal: &mut RestartJournal,
    now_secs: u64,
    limits: RestartJournalLimits,
) -> Result<()> {
    journal.compact(limits, now_secs);
    if !journal.enforce_serialized_cap(limits) {
        bail!(
            "restart journal live snapshot exceeds per-document cap ({} bytes); refusing to drop resumable state",
            limits.document_byte_cap
        );
    }
    let payload_json = serde_json::to_string(journal).context("encode restart journal")?;
    agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
        conn,
        &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
            document_hash: document_hash.to_string(),
            state_kind: RESTART_JOURNAL_STATE_KIND.to_string(),
            canonical_path: canonical_path.to_string(),
            payload_json,
            updated_at_secs: now_secs,
        },
    )?;
    enforce_global_cap(conn, limits)
}

pub fn record_main_phase(
    file: &Path,
    harness: &str,
    owner: &str,
    resume_id: Option<&str>,
    phase: MainAgentPhase,
    now_secs: u64,
) -> Result<bool> {
    record_main_phase_inner(file, harness, owner, resume_id, phase, now_secs, None)
}

/// Advance the main checkpoint only while `owner` still owns the journal row.
/// This fences a draining predecessor from marking a replacement supervisor's
/// main turn complete.
pub fn record_main_phase_if_owner(
    file: &Path,
    harness: &str,
    owner: &str,
    resume_id: Option<&str>,
    phase: MainAgentPhase,
    now_secs: u64,
) -> Result<bool> {
    record_main_phase_inner(
        file,
        harness,
        owner,
        resume_id,
        phase,
        now_secs,
        Some(owner),
    )
}

fn record_main_phase_inner(
    file: &Path,
    harness: &str,
    owner: &str,
    resume_id: Option<&str>,
    phase: MainAgentPhase,
    now_secs: u64,
    expected_owner: Option<&str>,
) -> Result<bool> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(false);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let limits = configured_limits();
        let mut journal = load_from_conn(&conn, &document_hash)?;
        if let Some(expected_owner) = expected_owner
            && journal.main.as_ref().map(|main| main.owner.as_str()) != Some(expected_owner)
        {
            return Ok(false);
        }
        journal.record_main(
            MainAgentCheckpoint {
                harness: harness.to_string(),
                owner: owner.to_string(),
                resume_id: resume_id.map(str::to_string),
                boot_id: current_boot_id(),
                phase,
                updated_at_secs: now_secs,
            },
            limits,
        );
        store_with_conn(
            &conn,
            &document_hash,
            &canonical_path,
            &mut journal,
            now_secs,
            limits,
        )?;
        Ok(true)
    })();
    finish_transaction(&conn, result)
}

/// Refresh the compact worker snapshot after a claim-ledger transition. This
/// runs inside the queue claim transaction so a restart cannot observe a newer
/// claim with an older restart projection.
pub(crate) fn sync_claims_with_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
    canonical_path: &str,
    ledger: &QueueClaimLedger,
    now_secs: u64,
) -> Result<()> {
    let limits = configured_limits();
    let mut journal = load_from_conn(conn, document_hash)?;
    let boot_id = current_boot_id();
    let active_keys: std::collections::HashSet<(String, u64)> = ledger
        .claims
        .iter()
        .map(|claim| (identity_hash(&claim.identity), claim.generation))
        .collect();
    let completed = journal
        .workers
        .iter()
        .filter(|checkpoint| {
            !active_keys.contains(&(checkpoint.identity_hash.clone(), checkpoint.generation))
                && !checkpoint.phase.terminal()
        })
        .cloned()
        .map(|mut checkpoint| {
            checkpoint.phase = WorkerPhase::Completed;
            checkpoint.updated_at_secs = now_secs;
            checkpoint
        })
        .collect::<Vec<_>>();
    for checkpoint in completed {
        journal.record_worker(checkpoint, limits);
    }
    for claim in &ledger.claims {
        let key = identity_hash(&claim.identity);
        let previous = journal.workers.iter().find(|checkpoint| {
            checkpoint.identity_hash == key && checkpoint.generation == claim.generation
        });
        let checkpoint = WorkerCheckpoint {
            identity_hash: key,
            generation: claim.generation,
            owner: claim.owner.clone(),
            worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
            // Preserve the originating boot for an existing attempt. A cold
            // reader must not make an orphan look reattached merely by loading it.
            boot_id: previous
                .map(|checkpoint| checkpoint.boot_id.clone())
                .unwrap_or_else(|| boot_id.clone()),
            phase: worker_phase(claim.state),
            artifact: previous.and_then(|checkpoint| checkpoint.artifact.clone()),
            updated_at_secs: now_secs,
        };
        journal.record_worker(checkpoint, limits);
    }
    store_with_conn(
        conn,
        document_hash,
        canonical_path,
        &mut journal,
        now_secs,
        limits,
    )
}

/// Cold-start reconciliation joins the compact claim snapshot with durable Git
/// lineage, then returns typed recovery actions. It does not mutate claims or
/// replay effects; those remain generation-fenced coordinator operations.
pub fn reconcile(file: &Path, now_secs: u64) -> Result<RestartRecoveryProjection> {
    let mut journal = load(file)?;
    attach_lineage_artifacts(file, &mut journal)?;
    let limits = configured_limits();
    journal.compact(limits, now_secs);
    let projection = journal.reconcile(&current_boot_id());
    if let Some((root, document_hash, canonical_path)) = state_identity(file)? {
        let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
        store_with_conn(
            &conn,
            &document_hash,
            &canonical_path,
            &mut journal,
            now_secs,
            limits,
        )?;
    }
    Ok(projection)
}

/// One compact operator-visible status line for startup logs/editor surfaces.
pub fn recovery_status_line(projection: &RestartRecoveryProjection) -> Option<String> {
    let main = projection
        .main
        .as_ref()
        .map(|main| format!("main={:?}", main.action));
    let worker_counts = projection.workers.iter().fold(
        std::collections::BTreeMap::<String, usize>::new(),
        |mut counts, worker| {
            *counts.entry(format!("{:?}", worker.action)).or_default() += 1;
            counts
        },
    );
    if main.is_none() && worker_counts.is_empty() {
        return None;
    }
    let workers = worker_counts
        .into_iter()
        .map(|(action, count)| format!("{action}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    Some(format!(
        "restart recovery {} workers=[{}]",
        main.unwrap_or_else(|| "main=None".to_string()),
        workers
    ))
}

fn attach_lineage_artifacts(file: &Path, journal: &mut RestartJournal) -> Result<()> {
    let lineage = crate::queue_lineage::load(file)?;
    for worker in &mut journal.workers {
        let Some(attempt) = lineage.attempts.iter().find(|attempt| {
            identity_hash(&attempt.identity) == worker.identity_hash
                && attempt.generation == worker.generation
        }) else {
            continue;
        };
        let Some(event) = attempt.events.iter().rev().find(|event| {
            event.source_sha.is_some()
                || event.worktree_path.is_some()
                || event.repo_path.is_some()
                || event.source_repo_path.is_some()
        }) else {
            continue;
        };
        let locator = event
            .worktree_path
            .as_deref()
            .or(event.source_repo_path.as_deref())
            .or(event.repo_path.as_deref())
            .unwrap_or("queue_lineage");
        worker.artifact = Some(ArtifactReference {
            locator_hash: agent_doc_hash::content_hash(locator),
            git_commit: event.source_sha.clone(),
        });
    }
    Ok(())
}

fn identity_hash(identity: &agent_doc_element_queue::QueueItemIdentity) -> String {
    let encoded = serde_json::to_string(identity).unwrap_or_else(|_| format!("{identity:?}"));
    agent_doc_hash::content_hash(&encoded)
}

fn worker_phase(state: QueueClaimState) -> WorkerPhase {
    match state {
        QueueClaimState::Claimed => WorkerPhase::Dispatching,
        QueueClaimState::Running => WorkerPhase::Running,
        QueueClaimState::Reported => WorkerPhase::Reported,
        QueueClaimState::Lost => WorkerPhase::Lost,
        QueueClaimState::QuotaBlocked => WorkerPhase::QuotaBlocked,
    }
}

fn enforce_global_cap(
    conn: &agent_doc_sqlite::state_store::Connection,
    limits: RestartJournalLimits,
) -> Result<()> {
    let mut records = agent_doc_sqlite::state_store::list_queue_document_state_from_db(
        conn,
        RESTART_JOURNAL_STATE_KIND,
    )?;
    let mut total: usize = records.iter().map(|record| record.payload_json.len()).sum();
    if total <= limits.global_byte_cap {
        return Ok(());
    }
    records.sort_by_key(|record| record.updated_at_secs);
    for record in records {
        if total <= limits.global_byte_cap {
            break;
        }
        let Ok(journal) = serde_json::from_str::<RestartJournal>(&record.payload_json) else {
            continue;
        };
        let terminal = journal
            .main
            .as_ref()
            .is_none_or(|main| main.phase.terminal())
            && journal.workers.iter().all(|worker| worker.phase.terminal());
        if !terminal {
            continue;
        }
        agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
            conn,
            &record.document_hash,
            RESTART_JOURNAL_STATE_KIND,
        )?;
        total = total.saturating_sub(record.payload_json.len());
    }
    if total > limits.global_byte_cap {
        bail!(
            "restart journal live set exceeds global cap ({} bytes); refusing to delete resumable documents",
            limits.global_byte_cap
        );
    }
    Ok(())
}

fn finish_transaction<T>(
    conn: &agent_doc_sqlite::state_store::Connection,
    result: Result<T>,
) -> Result<T> {
    match result {
        Ok(value) => {
            conn.execute_batch("COMMIT")?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK") {
                eprintln!("[restart-journal] WARNING: rollback failed after {err:#}: {rollback}");
            }
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::restart_recovery::{
        DEFAULT_DOCUMENT_BYTE_CAP, DEFAULT_GLOBAL_BYTE_CAP, RecoveryAction,
    };

    #[test]
    fn defaults_are_explicit_and_bounded() {
        let limits = RestartJournalLimits::default();
        assert_eq!(limits.document_byte_cap, DEFAULT_DOCUMENT_BYTE_CAP);
        assert_eq!(limits.global_byte_cap, DEFAULT_GLOBAL_BYTE_CAP);
        assert!(limits.document_byte_cap < limits.global_byte_cap);
    }

    #[test]
    fn boot_change_resumes_main_without_replaying_worker_effects() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        std::fs::create_dir_all(dir.path().join(".agent-doc"))?;
        let file = dir.path().join("session.md");
        std::fs::write(&file, "# session\n")?;
        // Store a prior-boot checkpoint without mutating process-global env.
        let (root, document_hash, canonical_path) = state_identity(&file)?.unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
        let mut journal = RestartJournal::default();
        journal.record_main(
            MainAgentCheckpoint {
                harness: "codex".into(),
                owner: "supervisor:old".into(),
                resume_id: Some("thread-1".into()),
                boot_id: "prior-boot".into(),
                phase: MainAgentPhase::Running,
                updated_at_secs: 1,
            },
            RestartJournalLimits::default(),
        );
        store_with_conn(
            &conn,
            &document_hash,
            &canonical_path,
            &mut journal,
            1,
            RestartJournalLimits::default(),
        )?;
        let projection = reconcile(&file, 2)?;
        assert_eq!(
            projection.main.map(|main| main.action),
            Some(RecoveryAction::ResumeMain)
        );
        Ok(())
    }

    #[test]
    fn stale_main_owner_cannot_close_replacement_checkpoint() -> Result<()> {
        let dir = tempfile::TempDir::new()?;
        std::fs::create_dir_all(dir.path().join(".agent-doc"))?;
        let file = dir.path().join("session.md");
        std::fs::write(&file, "# session\n")?;

        assert!(record_main_phase(
            &file,
            "codex",
            "supervisor:new",
            Some("thread-1"),
            MainAgentPhase::Running,
            2,
        )?);
        assert!(!record_main_phase_if_owner(
            &file,
            "codex",
            "supervisor:old",
            Some("thread-1"),
            MainAgentPhase::Completed,
            3,
        )?);
        let journal = load(&file)?;
        let main = journal.main.expect("main checkpoint");
        assert_eq!(main.owner, "supervisor:new");
        assert_eq!(main.phase, MainAgentPhase::Running);
        Ok(())
    }

    #[test]
    fn global_cap_collects_oldest_terminal_document_but_preserves_live_state() -> Result<()> {
        let conn = agent_doc_sqlite::state_store::Connection::open_in_memory()?;
        agent_doc_sqlite::state_store::initialize_state_db(&conn)?;
        let limits = RestartJournalLimits {
            global_byte_cap: usize::MAX,
            ..RestartJournalLimits::default()
        };

        let terminal = |updated_at_secs| {
            let mut journal = RestartJournal::default();
            journal.record_main(
                MainAgentCheckpoint {
                    harness: "codex".into(),
                    owner: format!("supervisor:{updated_at_secs}"),
                    resume_id: Some(format!("terminal-{updated_at_secs}")),
                    boot_id: "old-boot".into(),
                    phase: MainAgentPhase::Completed,
                    updated_at_secs,
                },
                limits,
            );
            journal
        };
        let mut live = RestartJournal::default();
        live.record_main(
            MainAgentCheckpoint {
                harness: "codex".into(),
                owner: "supervisor:live".into(),
                resume_id: Some("live".into()),
                boot_id: "old-boot".into(),
                phase: MainAgentPhase::Running,
                updated_at_secs: 3,
            },
            limits,
        );
        let fixtures = [
            ("terminal-old", terminal(1), 1),
            ("terminal-new", terminal(2), 2),
            ("live", live, 3),
        ];
        let mut total = 0usize;
        let mut oldest_size = 0usize;
        for (document_hash, journal, updated_at_secs) in fixtures {
            let payload_json = serde_json::to_string(&journal)?;
            total += payload_json.len();
            if document_hash == "terminal-old" {
                oldest_size = payload_json.len();
            }
            agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                &conn,
                &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                    document_hash: document_hash.into(),
                    state_kind: RESTART_JOURNAL_STATE_KIND.into(),
                    canonical_path: format!("/{document_hash}.md"),
                    payload_json,
                    updated_at_secs,
                },
            )?;
        }

        enforce_global_cap(
            &conn,
            RestartJournalLimits {
                global_byte_cap: total - oldest_size,
                ..limits
            },
        )?;

        assert!(
            agent_doc_sqlite::state_store::load_queue_document_state_from_db(
                &conn,
                "terminal-old",
                RESTART_JOURNAL_STATE_KIND,
            )?
            .is_none()
        );
        assert!(
            agent_doc_sqlite::state_store::load_queue_document_state_from_db(
                &conn,
                "terminal-new",
                RESTART_JOURNAL_STATE_KIND,
            )?
            .is_some()
        );
        assert!(
            agent_doc_sqlite::state_store::load_queue_document_state_from_db(
                &conn,
                "live",
                RESTART_JOURNAL_STATE_KIND,
            )?
            .is_some()
        );
        Ok(())
    }
}

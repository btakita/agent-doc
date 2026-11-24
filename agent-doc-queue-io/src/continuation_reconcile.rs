//! # Module: continuation_reconcile
//!
//! Commit-time queue-continuation reconcile as controller background work
//! (`#q-c71d106debc4`, preflightlag goal 2).
//!
//! ## Spec
//! - A successful closeout used to run
//!   [`crate::queue_continuation::reconcile_marker`] inline: prune closed
//!   claims and release receipts, detect whether the queue still owes a
//!   continuation, and write or clear the durable continuation marker. On
//!   haiven-dev `tasks/sdk/sdk-dev.md` that was 3.6-11.6 s of
//!   `commit_finalize_latency` (`queue:` phase), paid by every closeout and by
//!   preflight's `commit_previous_cycle`.
//! - [`reconcile_after_closeout`] instead publishes one pending-reconcile fact
//!   for the document (one `queue_document_state` row, a fresh generation per
//!   closeout) and notifies the running project controller, which runs the
//!   reconcile on its own per-document worker thread
//!   (`agent_doc_controller_io::document_maintenance::continuation_reconcile`).
//!   With no controller, one that refuses (an older binary, a generation that
//!   is handing off), or `AGENT_DOC_CONTINUATION_RECONCILE_HOST=inline`, the
//!   reconcile runs inline exactly as before.
//! - The Claude Stop hook reads the continuation marker right after closeout
//!   as its proof that the prior item closed cleanly. It first calls
//!   [`settle_pending`]: while a pending fact exists it waits (bounded) for the
//!   controller to finish, and if the fact is still pending it runs the
//!   reconcile itself. The marker the hook reads therefore always reflects the
//!   latest closeout, as it did when the commit wrote it inline.
//! - A run consumes the generation it read. A closeout that publishes a newer
//!   generation during a run leaves that generation pending, and the scheduler
//!   runs once more. Two runs racing (the Stop hook's inline settle and the
//!   controller worker) are safe: every step of the reconcile is idempotent.
//!
//! ## Agentic Contracts
//! - The marker never lags a closeout at the Stop hook's read.
//! - A failure to publish or to notify falls back to the inline reconcile; it
//!   is never fatal to closeout.
//!
//! ## Evals
//! - `a_pending_reconcile_is_run_and_consumed_by_generation`
//! - `a_newer_generation_survives_an_older_runs_consume`
//! - `settle_runs_the_pending_reconcile_inline_when_no_worker_finishes`
//! - `settle_returns_at_once_when_nothing_is_pending`
//! - `closeout_without_a_controller_reconciles_inline`

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent_doc_queue::queue_continuation::QueueContinuation;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const PENDING_STATE_KIND: &str = "continuation_reconcile_pending";

/// `inline` keeps the closeout-time reconcile in the committing process.
pub const CONTINUATION_RECONCILE_HOST_ENV: &str = "AGENT_DOC_CONTINUATION_RECONCILE_HOST";

/// How long the Stop hook waits for the controller's run before it runs the
/// pending reconcile itself.
pub const STOP_SETTLE_WAIT: Duration = Duration::from_secs(4);

const SETTLE_POLL: Duration = Duration::from_millis(25);

/// The pending-reconcile fact one closeout publishes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingContinuationReconcile {
    pub generation: String,
    pub source_command: String,
    pub requested_at_millis: u64,
}

/// What [`reconcile_after_closeout`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseoutReconcile {
    /// The controller accepted the reconcile; `outcome` is its scheduler
    /// outcome.
    Deferred { generation: String, outcome: String },
    /// The reconcile ran in this process.
    Inline(Option<QueueContinuation>),
}

/// What [`settle_pending`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleOutcome {
    /// No reconcile was pending.
    NothingPending,
    /// The controller's run consumed the pending reconcile while we waited.
    Background,
    /// The wait ran out and this process ran the pending reconcile.
    Inline,
}

impl SettleOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NothingPending => "nothing_pending",
            Self::Background => "background",
            Self::Inline => "inline",
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_generation() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{}-{nanos}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn state_identity(file: &Path) -> Result<Option<(PathBuf, String, String)>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize document path for hash: {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    Ok(Some((root, hash, canonical.to_string_lossy().into_owned())))
}

/// Publish a fresh pending reconcile for `file`. `Ok(None)` when `file` is not
/// inside an agent-doc project (nothing durable to reconcile).
pub fn publish_pending(
    file: &Path,
    source_command: &str,
) -> Result<Option<PendingContinuationReconcile>> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(None);
    };
    let pending = PendingContinuationReconcile {
        generation: new_generation(),
        source_command: source_command.to_string(),
        requested_at_millis: now_millis(),
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
        &conn,
        &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
            document_hash,
            state_kind: PENDING_STATE_KIND.to_string(),
            canonical_path,
            payload_json: serde_json::to_string(&pending)
                .context("serialize pending continuation reconcile")?,
            updated_at_secs: pending.requested_at_millis / 1000,
        },
    )?;
    Ok(Some(pending))
}

/// The pending reconcile for `file`, if any.
pub fn load_pending(file: &Path) -> Result<Option<PendingContinuationReconcile>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        &conn,
        &document_hash,
        PENDING_STATE_KIND,
    )?
    else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&record.payload_json).ok())
}

/// Consume `generation` if it is still `file`'s pending reconcile. A newer
/// generation published meanwhile stays pending. Returns whether it cleared.
pub fn consume_pending(file: &Path, generation: &str) -> Result<bool> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(false);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin pending continuation reconcile consume")?;
    let result = (|| {
        let current = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
            &conn,
            &document_hash,
            PENDING_STATE_KIND,
        )?
        .and_then(|record| {
            serde_json::from_str::<PendingContinuationReconcile>(&record.payload_json).ok()
        });
        if current.is_some_and(|current| current.generation != generation) {
            return Ok(false);
        }
        agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
            &conn,
            &document_hash,
            PENDING_STATE_KIND,
        )
    })();
    match result {
        Ok(cleared) => {
            conn.execute_batch("COMMIT")
                .context("commit pending continuation reconcile consume")?;
            Ok(cleared)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn inline_host_forced() -> bool {
    std::env::var(CONTINUATION_RECONCILE_HOST_ENV)
        .is_ok_and(|host| host.trim().eq_ignore_ascii_case("inline"))
}

/// Run the closeout-time continuation reconcile for `file` in the background
/// when a project controller serves it, inline otherwise (see the module docs).
pub fn reconcile_after_closeout(file: &Path, source_command: &str) -> CloseoutReconcile {
    reconcile_after_closeout_with(file, source_command, |file| {
        agent_doc_controller_io::project_controller::enqueue_queue_continuation_reconcile(file)
            .map(|receipt| receipt.outcome.as_str().to_string())
    })
}

/// [`reconcile_after_closeout`] with the controller notify injected.
pub fn reconcile_after_closeout_with(
    file: &Path,
    source_command: &str,
    notify: impl FnOnce(&Path) -> Result<String>,
) -> CloseoutReconcile {
    if inline_host_forced() {
        return CloseoutReconcile::Inline(crate::queue_continuation::reconcile_marker(
            file,
            source_command,
        ));
    }
    let started = Instant::now();
    let pending = match publish_pending(file, source_command) {
        Ok(Some(pending)) => pending,
        Ok(None) => {
            return CloseoutReconcile::Inline(crate::queue_continuation::reconcile_marker(
                file,
                source_command,
            ));
        }
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_continuation_reconcile_publish_failed file={} fallback=inline error={} (#q-c71d106debc4)",
                    file.display(),
                    format!("{error:#}").replace('\n', " | "),
                ),
            );
            return CloseoutReconcile::Inline(crate::queue_continuation::reconcile_marker(
                file,
                source_command,
            ));
        }
    };
    match notify(file) {
        Ok(outcome) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_continuation_reconcile_deferred file={} source={source_command} generation={} outcome={outcome} elapsed_us={} (#q-c71d106debc4)",
                    file.display(),
                    pending.generation,
                    started.elapsed().as_micros(),
                ),
            );
            CloseoutReconcile::Deferred {
                generation: pending.generation,
                outcome,
            }
        }
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_continuation_reconcile_controller_unavailable file={} fallback=inline elapsed_us={} error={} (#q-c71d106debc4)",
                    file.display(),
                    started.elapsed().as_micros(),
                    format!("{error:#}").replace('\n', " | "),
                ),
            );
            let continuation = crate::queue_continuation::reconcile_marker(file, source_command);
            if let Err(error) = consume_pending(file, &pending.generation) {
                eprintln!(
                    "[queue-continuation] WARNING: failed to consume the pending reconcile for {}: {error:#}",
                    file.display()
                );
            }
            CloseoutReconcile::Inline(continuation)
        }
    }
}

/// Run `file`'s pending reconcile, if any, and consume the generation it ran
/// for. Returns `Ok(false)` when nothing was pending. The controller's
/// per-document worker and [`settle_pending`] both call this.
pub fn run_pending(file: &Path, runner: &str) -> Result<bool> {
    let Some(pending) = load_pending(file)? else {
        return Ok(false);
    };
    let started = Instant::now();
    let continuation = crate::queue_continuation::reconcile_marker(file, &pending.source_command);
    let consumed = consume_pending(file, &pending.generation)?;
    let lag_ms = now_millis().saturating_sub(pending.requested_at_millis);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_continuation_reconcile_ran file={} runner={runner} source={} generation={} continuation={} consumed={consumed} elapsed_ms={} request_age_ms={lag_ms} (#q-c71d106debc4)",
            file.display(),
            pending.source_command,
            pending.generation,
            continuation.is_some(),
            started.elapsed().as_millis(),
        ),
    );
    if let Some(continuation) = continuation {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_continuation_required file={} head={}",
                file.display(),
                continuation.head_prompt.replace('\n', " ")
            ),
        );
    }
    Ok(true)
}

/// Make the continuation marker current before a reader relies on it: wait up
/// to `wait` for a pending reconcile to be consumed by the controller, then
/// run it here. Never fails: a reconcile error is logged and the reader goes
/// on with what the marker says.
pub fn settle_pending(file: &Path, wait: Duration, caller: &str) -> SettleOutcome {
    let started = Instant::now();
    let deadline = started + wait;
    let outcome = loop {
        match load_pending(file) {
            Ok(None) => {
                break if started.elapsed() < SETTLE_POLL {
                    SettleOutcome::NothingPending
                } else {
                    SettleOutcome::Background
                };
            }
            Ok(Some(_)) if Instant::now() < deadline => std::thread::sleep(SETTLE_POLL),
            Ok(Some(_)) | Err(_) => {
                if let Err(error) = run_pending(file, caller) {
                    eprintln!(
                        "[queue-continuation] WARNING: pending continuation reconcile failed for {}: {error:#}",
                        file.display()
                    );
                }
                break SettleOutcome::Inline;
            }
        }
    };
    if outcome != SettleOutcome::NothingPending {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_continuation_reconcile_settled file={} caller={caller} outcome={} waited_ms={} (#q-c71d106debc4)",
                file.display(),
                outcome.as_str(),
                started.elapsed().as_millis(),
            ),
        );
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::continuation_marker::load_continuation_marker;

    fn go_queue_doc(dir: &Path, head: &str) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/snapshots")).unwrap();
        let doc = dir.join("task.md");
        let content = format!(
            "---\nsession: sid\nagent_doc_format: template\nqueue: start\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue auto go -->\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    #[test]
    fn a_pending_reconcile_is_run_and_consumed_by_generation() {
        let dir = tempfile::tempdir().unwrap();
        let doc = go_queue_doc(dir.path(), "do [#a]");
        let pending = publish_pending(&doc, "commit").unwrap().unwrap();
        assert_eq!(load_pending(&doc).unwrap(), Some(pending));
        assert!(load_continuation_marker(&doc).unwrap().is_none());

        assert!(run_pending(&doc, "test").unwrap());

        assert!(load_pending(&doc).unwrap().is_none());
        let marker = load_continuation_marker(&doc).unwrap().expect("marker");
        assert_eq!(marker.head_id.as_deref(), Some("a"));
        assert!(!run_pending(&doc, "test").unwrap(), "nothing left to run");
    }

    #[test]
    fn a_newer_generation_survives_an_older_runs_consume() {
        let dir = tempfile::tempdir().unwrap();
        let doc = go_queue_doc(dir.path(), "do [#a]");
        let older = publish_pending(&doc, "commit").unwrap().unwrap();
        let newer = publish_pending(&doc, "commit").unwrap().unwrap();
        assert_ne!(older.generation, newer.generation);

        assert!(!consume_pending(&doc, &older.generation).unwrap());
        assert_eq!(load_pending(&doc).unwrap(), Some(newer.clone()));
        assert!(consume_pending(&doc, &newer.generation).unwrap());
        assert!(load_pending(&doc).unwrap().is_none());
    }

    #[test]
    fn settle_runs_the_pending_reconcile_inline_when_no_worker_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let doc = go_queue_doc(dir.path(), "do [#a]");
        let deferred = reconcile_after_closeout_with(&doc, "commit", |_| Ok("scheduled".into()));
        assert!(matches!(deferred, CloseoutReconcile::Deferred { .. }));
        assert!(
            load_continuation_marker(&doc).unwrap().is_none(),
            "a deferred closeout leaves the marker to the background run"
        );

        let outcome = settle_pending(&doc, Duration::from_millis(60), "test");

        assert_eq!(outcome, SettleOutcome::Inline);
        assert!(load_pending(&doc).unwrap().is_none());
        assert!(
            load_continuation_marker(&doc).unwrap().is_some(),
            "the Stop hook reads a marker that reflects the closeout"
        );
    }

    #[test]
    fn settle_returns_at_once_when_nothing_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        let doc = go_queue_doc(dir.path(), "do [#a]");
        let started = Instant::now();
        assert_eq!(
            settle_pending(&doc, Duration::from_secs(5), "test"),
            SettleOutcome::NothingPending
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(load_continuation_marker(&doc).unwrap().is_none());
    }

    #[test]
    fn settle_observes_a_background_run_that_finishes_inside_the_wait() {
        let dir = tempfile::tempdir().unwrap();
        let doc = go_queue_doc(dir.path(), "do [#a]");
        let deferred = reconcile_after_closeout_with(&doc, "commit", |_| Ok("scheduled".into()));
        assert!(matches!(deferred, CloseoutReconcile::Deferred { .. }));
        let worker_doc = doc.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            run_pending(&worker_doc, "worker").unwrap()
        });

        let outcome = settle_pending(&doc, Duration::from_secs(5), "test");

        assert!(worker.join().unwrap());
        assert_eq!(outcome, SettleOutcome::Background);
        assert!(load_continuation_marker(&doc).unwrap().is_some());
    }

    #[test]
    fn closeout_without_a_controller_reconciles_inline() {
        let dir = tempfile::tempdir().unwrap();
        let doc = go_queue_doc(dir.path(), "do [#a]");

        let outcome = reconcile_after_closeout_with(&doc, "commit", |_| {
            anyhow::bail!("no project controller is running")
        });

        let CloseoutReconcile::Inline(Some(continuation)) = outcome else {
            panic!("expected an inline reconcile, got {outcome:?}");
        };
        assert_eq!(continuation.head_id.as_deref(), Some("a"));
        assert!(load_continuation_marker(&doc).unwrap().is_some());
        assert!(load_pending(&doc).unwrap().is_none());
    }
}

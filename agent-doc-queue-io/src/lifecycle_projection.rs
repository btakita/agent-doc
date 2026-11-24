//! # Module: lifecycle_projection
//!
//! Queue lifecycle glyph transitions projected at once by the project
//! controller (`#q-9f8f5f60c2c3`, preflightlag goal 4).
//!
//! ## Spec
//! - A queue row's lifecycle glyph (`🟡` claimed, `🚧` running, `⚠️` lost,
//!   `☑️` reported, ...) is a projection of the claim ledger. `queue claim`,
//!   `queue start`, `queue release` and `queue claim --refresh` repaint the
//!   row themselves. A transition that changes only the ledger did not: a
//!   `queue freeze`, a SubagentStop hook's freeze, a Codex worker's terminal
//!   freeze, and a worker's proof of life (`#waitingfreezelive`). No document
//!   edge follows such a transition, so the supervisor's delivery-quiet
//!   self-heal never woke for it, and the row kept its old glyph until the
//!   next cycle (live haiven-dev: a `🚧` row frozen at 03:07:51 still showed
//!   `🚧` when its report landed 13 s later; agent-loop pflag, 19 s).
//! - [`project_after_transition`] publishes the transitioned claim item into
//!   one pending-projection fact per document (`queue_document_state`
//!   `lifecycle_projection_pending`: the items, a fresh generation per
//!   publish) and notifies the running project controller
//!   (`queue_lifecycle_projection_enqueue`, existing-only, 1 s bound). The
//!   controller runs [`run_pending`] on a per-document worker
//!   (`agent_doc_controller_io::document_maintenance::lifecycle_projection`,
//!   10 ms debounce) that repaints exactly those items' rows
//!   ([`crate::queue_cmd`]'s identity-scoped claim projection, not a whole
//!   document rescan) and consumes the generation it ran for.
//! - The hook only pays the publish and the notify. With no controller (or
//!   one that refuses), the caller's [`LifecycleProjectionFallback`] decides:
//!   a CLI command projects inline, as `queue start` does; a hook leaves the
//!   fact for the next projection pass, exactly the old behavior.
//!   `AGENT_DOC_LIFECYCLE_PROJECTION_HOST=inline` forces the inline path.
//! - A run consumes the generation it read; a transition published during the
//!   run stays pending and the scheduler runs once more. The projection is
//!   identity-scoped and idempotent, so a duplicate run is harmless.
//!
//! ## Agentic Contracts
//! - A ledger transition is never failed or reverted by its projection: every
//!   publish, notify or projection failure is logged and falls back.
//! - The worker holds the queue apply capability as a background owner, so a
//!   foreground queue mutation waits behind it instead of failing closed.
//!
//! ## Evals
//! - `a_freeze_published_for_the_controller_is_painted_by_its_run`
//! - `a_newer_generation_survives_an_older_runs_consume`
//! - `pending_items_merge_without_duplicates`
//! - `a_cli_transition_without_a_controller_projects_inline`
//! - `a_hook_transition_without_a_controller_is_left_for_the_next_cycle`

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::queue_cmd::QueueRuntimeProjectionEffects;

const PENDING_STATE_KIND: &str = "lifecycle_projection_pending";

/// `inline` keeps the transition's projection in the transitioning process.
pub const LIFECYCLE_PROJECTION_HOST_ENV: &str = "AGENT_DOC_LIFECYCLE_PROJECTION_HOST";

/// How long the controller worker waits for a foreground queue mutation to
/// release the apply capability before its run fails and is retried.
const BACKGROUND_APPLY_WAIT: Duration = Duration::from_secs(10);

/// A pending fact keeps at most this many items; the oldest fall off (the
/// next whole projection pass still repaints them).
const MAX_PENDING_ITEMS: usize = 32;

/// One transitioned claim item awaiting its glyph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingLifecycleItem {
    pub item: String,
    pub source: String,
    pub requested_at_millis: u64,
}

/// The pending lifecycle projection of one document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingLifecycleProjection {
    pub generation: String,
    pub items: Vec<PendingLifecycleItem>,
}

/// What the transitioning process does when no controller takes the
/// projection.
#[derive(Clone, Copy)]
pub enum LifecycleProjectionFallback<'a> {
    /// Project in this process now (a CLI command).
    Inline(&'a dyn QueueRuntimeProjectionEffects),
    /// Leave the pending fact for the next projection pass (a hook, which
    /// must stay quick).
    NextCycle,
}

/// What [`project_after_transition`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionProjection {
    /// The controller accepted the projection; `outcome` is its scheduler
    /// outcome.
    Deferred { generation: String, outcome: String },
    /// The projection ran in this process; `changed` items were repainted.
    Inline { changed: usize },
    /// No controller took it and the caller left it for the next pass.
    LeftForNextCycle,
    /// Nothing durable to project (not inside an agent-doc project), or the
    /// projection failed; the transition itself stands.
    Skipped,
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

fn in_transaction<T>(
    conn: &agent_doc_sqlite::state_store::Connection,
    what: &str,
    body: impl FnOnce() -> Result<T>,
) -> Result<T> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .with_context(|| format!("begin {what}"))?;
    match body() {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .with_context(|| format!("commit {what}"))?;
            Ok(value)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn load_from(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<Option<PendingLifecycleProjection>> {
    Ok(
        agent_doc_sqlite::state_store::load_queue_document_state_from_db(
            conn,
            document_hash,
            PENDING_STATE_KIND,
        )?
        .and_then(|record| serde_json::from_str(&record.payload_json).ok()),
    )
}

/// Add `item` to `file`'s pending lifecycle projection under a fresh
/// generation. `Ok(None)` when `file` is not inside an agent-doc project.
pub fn publish_pending(
    file: &Path,
    item: &str,
    source: &str,
) -> Result<Option<PendingLifecycleProjection>> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        return Ok(None);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let pending = in_transaction(&conn, "pending lifecycle projection publish", || {
        let mut items = load_from(&conn, &document_hash)?
            .map(|pending| pending.items)
            .unwrap_or_default();
        let now = now_millis();
        if !items.iter().any(|pending| pending.item == item) {
            items.push(PendingLifecycleItem {
                item: item.to_string(),
                source: source.to_string(),
                requested_at_millis: now,
            });
        }
        if items.len() > MAX_PENDING_ITEMS {
            items.drain(..items.len() - MAX_PENDING_ITEMS);
        }
        let pending = PendingLifecycleProjection {
            generation: new_generation(),
            items,
        };
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: document_hash.clone(),
                state_kind: PENDING_STATE_KIND.to_string(),
                canonical_path,
                payload_json: serde_json::to_string(&pending)
                    .context("serialize pending lifecycle projection")?,
                updated_at_secs: now / 1000,
            },
        )?;
        Ok(pending)
    })?;
    Ok(Some(pending))
}

/// The pending lifecycle projection for `file`, if any.
pub fn load_pending(file: &Path) -> Result<Option<PendingLifecycleProjection>> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from(&conn, &document_hash)
}

/// Consume `generation` if it is still `file`'s pending projection. A newer
/// generation published meanwhile stays pending. Returns whether it cleared.
pub fn consume_pending(file: &Path, generation: &str) -> Result<bool> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(false);
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    in_transaction(&conn, "pending lifecycle projection consume", || {
        if load_from(&conn, &document_hash)?.is_some_and(|current| current.generation != generation)
        {
            return Ok(false);
        }
        agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
            &conn,
            &document_hash,
            PENDING_STATE_KIND,
        )
    })
}

fn inline_host_forced() -> bool {
    std::env::var(LIFECYCLE_PROJECTION_HOST_ENV)
        .is_ok_and(|host| host.trim().eq_ignore_ascii_case("inline"))
}

fn one_line(error: &anyhow::Error) -> String {
    format!("{error:#}").replace('\n', " | ")
}

/// How [`run_pending`] takes the queue apply capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOwner {
    /// The controller worker: a background owner foreground mutations wait
    /// behind.
    Background,
    /// The transitioning CLI process itself.
    Foreground,
}

/// Project the transitioned claim `item` onto its row: hand it to the project
/// controller when one serves the document, otherwise apply `fallback` (see
/// the module docs). Never fails; the ledger transition already stands.
pub fn project_after_transition(
    file: &Path,
    item: &str,
    source: &str,
    fallback: LifecycleProjectionFallback<'_>,
) -> TransitionProjection {
    project_after_transition_with(file, item, source, fallback, |file| {
        agent_doc_controller_io::project_controller::enqueue_queue_lifecycle_projection(
            file, source,
        )
        .map(|receipt| receipt.outcome.as_str().to_string())
    })
}

/// [`project_after_transition`] with the controller notify injected.
pub fn project_after_transition_with(
    file: &Path,
    item: &str,
    source: &str,
    fallback: LifecycleProjectionFallback<'_>,
    notify: impl FnOnce(&Path) -> Result<String>,
) -> TransitionProjection {
    let started = Instant::now();
    let pending = match publish_pending(file, item, source) {
        Ok(Some(pending)) => pending,
        Ok(None) => return TransitionProjection::Skipped,
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_lifecycle_projection_publish_failed file={} source={source} error={} (#q-9f8f5f60c2c3)",
                    file.display(),
                    one_line(&error),
                ),
            );
            return run_fallback(file, item, source, fallback);
        }
    };
    let notified =
        if inline_host_forced() && matches!(fallback, LifecycleProjectionFallback::Inline(_)) {
            Err(anyhow::anyhow!("{LIFECYCLE_PROJECTION_HOST_ENV}=inline"))
        } else {
            notify(file)
        };
    match notified {
        Ok(outcome) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_lifecycle_projection_deferred file={} source={source} item_bytes={} generation={} outcome={outcome} elapsed_us={} (#q-9f8f5f60c2c3)",
                    file.display(),
                    item.len(),
                    pending.generation,
                    started.elapsed().as_micros(),
                ),
            );
            TransitionProjection::Deferred {
                generation: pending.generation,
                outcome,
            }
        }
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_lifecycle_projection_controller_unavailable file={} source={source} fallback={} elapsed_us={} error={} (#q-9f8f5f60c2c3)",
                    file.display(),
                    match fallback {
                        LifecycleProjectionFallback::Inline(_) => "inline",
                        LifecycleProjectionFallback::NextCycle => "next_cycle",
                    },
                    started.elapsed().as_micros(),
                    one_line(&error),
                ),
            );
            match fallback {
                LifecycleProjectionFallback::Inline(effects) => {
                    match run_pending(effects, file, "inline", ApplyOwner::Foreground) {
                        Ok(changed) => TransitionProjection::Inline {
                            changed: changed.unwrap_or(0),
                        },
                        Err(error) => {
                            log_run_failed(file, "inline", &error);
                            TransitionProjection::Skipped
                        }
                    }
                }
                LifecycleProjectionFallback::NextCycle => TransitionProjection::LeftForNextCycle,
            }
        }
    }
}

fn run_fallback(
    file: &Path,
    item: &str,
    source: &str,
    fallback: LifecycleProjectionFallback<'_>,
) -> TransitionProjection {
    let LifecycleProjectionFallback::Inline(effects) = fallback else {
        return TransitionProjection::LeftForNextCycle;
    };
    match crate::queue_cmd::project_claim_item_runtime_state_with_effects(
        effects, file, item, false,
    ) {
        Ok(changed) => TransitionProjection::Inline {
            changed: usize::from(changed),
        },
        Err(error) => {
            log_run_failed(file, source, &error);
            TransitionProjection::Skipped
        }
    }
}

fn log_run_failed(file: &Path, runner: &str, error: &anyhow::Error) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_lifecycle_projection_failed file={} runner={runner} error={} (#q-9f8f5f60c2c3)",
            file.display(),
            one_line(error),
        ),
    );
}

/// Project `file`'s pending lifecycle items onto their rows and consume the
/// generation it ran for. `Ok(None)` when nothing was pending, otherwise the
/// number of items whose row changed. The controller's per-document worker
/// and the inline fallback both call this.
pub fn run_pending(
    effects: &(impl QueueRuntimeProjectionEffects + ?Sized),
    file: &Path,
    runner: &str,
    owner: ApplyOwner,
) -> Result<Option<usize>> {
    let Some(pending) = load_pending(file)? else {
        return Ok(None);
    };
    let started = Instant::now();
    let guard = match owner {
        ApplyOwner::Background => crate::queue_edit_owner::QueueEditGuard::acquire_background(
            file,
            "lifecycle-projection",
            BACKGROUND_APPLY_WAIT,
        )?,
        ApplyOwner::Foreground => crate::queue_edit_owner::QueueEditGuard::acquire(file)?,
    };
    let acquired_ms = started.elapsed().as_millis();
    let items = pending
        .items
        .iter()
        .map(|pending| pending.item.clone())
        .collect::<Vec<_>>();
    let changed = crate::queue_cmd::project_claim_items_runtime_state_under_apply_guard(
        effects, file, &items, &guard,
    )?;
    drop(guard);
    let consumed = consume_pending(file, &pending.generation)?;
    let now = now_millis();
    let oldest_request_age_ms = pending
        .items
        .iter()
        .map(|item| now.saturating_sub(item.requested_at_millis))
        .max()
        .unwrap_or(0);
    let sources = pending
        .items
        .iter()
        .map(|item| item.source.as_str())
        .collect::<Vec<_>>()
        .join(",");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_lifecycle_projection_ran file={} runner={runner} sources={sources} items={} changed={changed} consumed={consumed} apply_wait_ms={acquired_ms} elapsed_ms={} request_age_ms={oldest_request_age_ms} (#q-9f8f5f60c2c3)",
            file.display(),
            items.len(),
            started.elapsed().as_millis(),
        ),
    );
    Ok(Some(changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Disk-backed projection effects: the document is the file.
    #[derive(Default)]
    struct DiskEffects {
        writes: RefCell<usize>,
    }

    impl QueueRuntimeProjectionEffects for DiskEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            *self.writes.borrow_mut() += 1;
            std::fs::write(file, target_content)?;
            Ok(())
        }
    }

    fn queue_doc(dir: &Path, heads: &[&str]) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let doc = dir.join("task.md");
        let rows = heads
            .iter()
            .map(|head| format!("- {head}\n"))
            .collect::<String>();
        std::fs::write(
            &doc,
            format!(
                "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue -->\n{rows}<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        doc
    }

    fn running(doc: &Path, item: &str, owner: &str) {
        let effects = DiskEffects::default();
        let (_, claim) =
            crate::queue_cmd::claim_with_runtime_projection(&effects, doc, item, owner, 600)
                .unwrap();
        crate::queue_claim::start_attempt(
            doc,
            item,
            owner,
            &format!("worker:{owner}"),
            600,
            claim.generation,
        )
        .unwrap();
        crate::queue_cmd::project_claim_item_runtime_state_with_effects(
            &effects,
            doc,
            &claim.item_text,
            false,
        )
        .unwrap();
    }

    fn row(doc: &Path, needle: &str) -> String {
        std::fs::read_to_string(doc)
            .unwrap()
            .lines()
            .find(|line| line.contains(needle))
            .unwrap_or_default()
            .to_string()
    }

    fn lost() -> &'static str {
        agent_doc_document::queue_projection::QueueLifecycleMarker::Lost.prefix()
    }

    fn running_marker() -> &'static str {
        agent_doc_document::queue_projection::QueueLifecycleMarker::Running.prefix()
    }

    #[test]
    fn a_freeze_published_for_the_controller_is_painted_by_its_run() {
        let dir = tempfile::tempdir().unwrap();
        let doc = queue_doc(dir.path(), &["do [#a]", "do [#b]"]);
        running(&doc, "do [#a]", "subagent:a");
        assert!(
            row(&doc, "[#a]").contains(running_marker()),
            "{}",
            row(&doc, "[#a]")
        );

        crate::queue_claim::freeze_attempt(&doc, "do [#a]", Some(1), "stopped").unwrap();
        let deferred = project_after_transition_with(
            &doc,
            "do [#a]",
            "test_freeze",
            LifecycleProjectionFallback::NextCycle,
            |_| Ok("scheduled".to_string()),
        );
        assert!(
            matches!(deferred, TransitionProjection::Deferred { .. }),
            "{deferred:?}"
        );
        assert!(
            row(&doc, "[#a]").contains(running_marker()),
            "the transition itself paints nothing: {}",
            row(&doc, "[#a]")
        );

        let effects = DiskEffects::default();
        assert_eq!(
            run_pending(&effects, &doc, "test", ApplyOwner::Background).unwrap(),
            Some(1)
        );
        let painted = row(&doc, "[#a]");
        assert!(painted.contains(lost()), "{painted}");
        assert!(!painted.contains(running_marker()), "{painted}");
        assert!(
            !row(&doc, "[#b]").contains(lost()),
            "only the transitioned row changes"
        );
        assert!(load_pending(&doc).unwrap().is_none());
        assert_eq!(
            run_pending(&effects, &doc, "test", ApplyOwner::Background).unwrap(),
            None,
            "nothing left to run"
        );
    }

    #[test]
    fn a_newer_generation_survives_an_older_runs_consume() {
        let dir = tempfile::tempdir().unwrap();
        let doc = queue_doc(dir.path(), &["do [#a]"]);
        let older = publish_pending(&doc, "do [#a]", "one").unwrap().unwrap();
        let newer = publish_pending(&doc, "do [#a]", "two").unwrap().unwrap();
        assert_ne!(older.generation, newer.generation);

        assert!(!consume_pending(&doc, &older.generation).unwrap());
        assert_eq!(load_pending(&doc).unwrap(), Some(newer.clone()));
        assert!(consume_pending(&doc, &newer.generation).unwrap());
        assert!(load_pending(&doc).unwrap().is_none());
    }

    #[test]
    fn pending_items_merge_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let doc = queue_doc(dir.path(), &["do [#a]", "do [#b]"]);
        publish_pending(&doc, "do [#a]", "freeze").unwrap();
        publish_pending(&doc, "do [#b]", "freeze").unwrap();
        let pending = publish_pending(&doc, "do [#a]", "liveness")
            .unwrap()
            .unwrap();
        let items = pending
            .items
            .iter()
            .map(|item| (item.item.as_str(), item.source.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(items, vec![("do [#a]", "freeze"), ("do [#b]", "freeze")]);
    }

    #[test]
    fn a_cli_transition_without_a_controller_projects_inline() {
        let dir = tempfile::tempdir().unwrap();
        let doc = queue_doc(dir.path(), &["do [#a]"]);
        running(&doc, "do [#a]", "subagent:a");
        crate::queue_claim::freeze_attempt(&doc, "do [#a]", Some(1), "stopped").unwrap();

        let effects = DiskEffects::default();
        let outcome = project_after_transition_with(
            &doc,
            "do [#a]",
            "queue_freeze",
            LifecycleProjectionFallback::Inline(&effects),
            |_| anyhow::bail!("no project controller is running"),
        );

        assert_eq!(outcome, TransitionProjection::Inline { changed: 1 });
        assert!(row(&doc, "[#a]").contains(lost()), "{}", row(&doc, "[#a]"));
        assert!(load_pending(&doc).unwrap().is_none());
    }

    #[test]
    fn a_hook_transition_without_a_controller_is_left_for_the_next_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let doc = queue_doc(dir.path(), &["do [#a]"]);
        running(&doc, "do [#a]", "subagent:a");
        crate::queue_claim::freeze_attempt(&doc, "do [#a]", Some(1), "stopped").unwrap();

        let outcome = project_after_transition_with(
            &doc,
            "do [#a]",
            "subagent_stop_freeze",
            LifecycleProjectionFallback::NextCycle,
            |_| anyhow::bail!("no project controller is running"),
        );

        assert_eq!(outcome, TransitionProjection::LeftForNextCycle);
        assert!(
            row(&doc, "[#a]").contains(running_marker()),
            "a hook never pays the projection: {}",
            row(&doc, "[#a]")
        );
        assert!(
            load_pending(&doc).unwrap().is_some(),
            "the fact waits for the next run"
        );
    }

    #[test]
    fn a_background_owner_holds_a_lease_foreground_mutations_wait_behind() {
        let dir = tempfile::tempdir().unwrap();
        let doc = queue_doc(dir.path(), &["do [#a]"]);
        let guard = crate::queue_edit_owner::QueueEditGuard::acquire_background(
            &doc,
            "lifecycle-projection",
            Duration::from_secs(1),
        )
        .unwrap();
        let lease =
            crate::queue_edit_owner::read_queue_edit_owner_lease(&doc.to_string_lossy()).unwrap();
        assert!(
            lease
                .holder
                .starts_with(crate::queue_edit_owner::BACKGROUND_QUEUE_EDIT_OWNER_PREFIX),
            "{lease:?}"
        );
        let waiter_doc = doc.clone();
        let waiter = std::thread::spawn(move || {
            crate::queue_edit_owner::QueueEditGuard::acquire(&waiter_doc).map(|_| ())
        });
        std::thread::sleep(Duration::from_millis(100));
        drop(guard);
        waiter
            .join()
            .unwrap()
            .expect("a foreground mutation waits behind the background owner");
        assert!(
            crate::queue_edit_owner::read_queue_edit_owner_lease(&doc.to_string_lossy()).is_none()
        );
    }
}

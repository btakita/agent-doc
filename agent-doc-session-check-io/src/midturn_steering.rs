//! Mid-turn operator steering delivery (`#midturn-steering`).
//!
//! The deterministic core lives in
//! [`agent_doc_document_realtime::midturn_steering`]; this module owns the
//! durable watermark, the cheap observation gate, and the harness hook
//! envelope.
//!
//! **Why a stateless compare and not a Lazily actor edge.** The delivery
//! surface is a harness `PostToolUse` event: a one-shot process spawned after
//! every tool call, with no long-lived scope of its own. That makes it the
//! narrow actorless trigger `#lazily-reactive-first` allows, but not a second
//! document authority: an attached editor's current text still comes from the
//! controller-owned CRDT projection. Detached documents retain the cheap file-
//! stat gate; attached documents resolve the live projection on every event so
//! an unsaved operator edit invalidates the comparison even while disk metadata
//! is unchanged. The hook compares that observation against durable,
//! cycle-scoped state (the watermark preflight seeds in `state.db`) and writes
//! the advanced watermark back. The cycle itself is consulted only on the rare
//! path where something is ready to surface, which is also where a closed or
//! superseded cycle silences the watermark for good.
//!
//! Storage: `state.db` `project_runtime_state`, one base record per document
//! (seeded by preflight) plus one progress record per consumer (`hook`, `cli`,
//! `follow`), so a polling CLI never steals steering from the in-turn hook.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;

use agent_doc_document_realtime::midturn_steering::{
    self as core, DEFAULT_STEERING_DEBOUNCE_MS, ObserveContext, SteeringItem, SteeringWatermark,
};

/// The in-turn harness hook consumer.
pub const CONSUMER_HOOK: &str = "hook";
/// `agent-doc steering <FILE>` polling consumer.
pub const CONSUMER_CLI: &str = "cli";
/// `agent-doc steering --follow <FILE>` stream consumer.
pub const CONSUMER_FOLLOW: &str = "follow";

/// Stable prefix so agents and tests can distinguish the reminder from
/// operator-authored mid-turn steering.
pub const STATUS_SYNC_CHECKPOINT_MARKER: &str = "[agent-doc] status-sync checkpoint";

/// What one observation produced for a consumer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SteeringReport {
    pub document: String,
    pub cycle_id: String,
    pub items: Vec<SteeringItem>,
    pub pending: usize,
    /// Observed at/after the turn boundary (the cycle no longer accepts
    /// mid-turn steering): items are framed for the next cycle.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub after_close: bool,
    /// When a held item's settle decision can next change (ms from now).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recheck_after_ms: Option<u64>,
}

impl SteeringReport {
    /// Agent-facing context, `None` when nothing is ready.
    pub fn render(&self) -> Option<String> {
        if self.after_close {
            core::render_closeout_steering_context(&self.document, &self.items, self.pending)
        } else {
            core::render_steering_context(&self.document, &self.items, self.pending)
        }
    }
}

fn state_key(kind: &str, file: &Path) -> String {
    agent_doc_queue_io::subagent_dispatch::midturn_steering_state_key(kind, file)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn project_root(file: &Path) -> Option<PathBuf> {
    agent_doc_fs::find_project_root(file)
}

/// Seed the cycle watermark from preflight's admitted document.
///
/// `current_item` is the queue prompt this cycle selected (if any) and
/// `session_presets` the preset names this cycle's prompt requested; both
/// scope later classification. Re-seeding the same cycle (re-entrant
/// preflight) replaces the base, and consumers re-initialize because their
/// recorded baseline hash no longer matches.
pub fn seed_for_cycle(
    file: &Path,
    cycle_id: &str,
    baseline: &str,
    current_item: Option<&str>,
    session_presets: Vec<String>,
) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let _nested_claim_steer = agent_doc_queue_io::queue_claim::steer_guard_for(file, baseline);
    let mut watermark = SteeringWatermark::seed(cycle_id, baseline, current_item, session_presets);
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    // Keep the previous cycle's seed queue for `queue_subagent_dispatch`: a
    // re-entrant preflight of the same cycle inherits it, a new cycle takes
    // the outgoing base's queue.
    let outgoing_base = load_watermark(&conn, &state_key("base", file))?;
    // `#claimedsteerwake`: what the agent channel last surfaced. The hook
    // consumer advances past the base as it surfaces; fall back to the base.
    let prior_acknowledged = match (
        &outgoing_base,
        load_watermark(&conn, &state_key(CONSUMER_HOOK, file))?,
    ) {
        (Some(base), _) if base.cycle_id == cycle_id => None,
        (Some(base), Some(hook)) if hook.cycle_id == base.cycle_id => Some(hook.acknowledged_queue),
        (Some(base), _) => Some(base.acknowledged_queue.clone()),
        (None, _) => None,
    };
    watermark.prior_cycle_queue = match outgoing_base {
        Some(existing) if existing.cycle_id == cycle_id => existing.prior_cycle_queue,
        Some(existing) => Some(existing.acknowledged_queue),
        None => None,
    };
    if let Some(prior) = prior_acknowledged {
        let owners = agent_doc_queue_io::queue_claim::claim_owners_for_content(file, baseline);
        if !owners.is_empty() {
            let carried = core::carry_unsurfaced_claimed_edits(&mut watermark, &prior, |text| {
                owners.contains_key(&agent_doc_queue::queue_claim::claim_identity(text))
            });
            if carried > 0 {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "midturn_steering_seed_carried_claimed_edits file={} cycle={cycle_id} count={carried} (#claimedsteerwake)",
                        file.display()
                    ),
                );
            }
        }
    }
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &state_key("base", file),
        &serde_json::to_string(&watermark)?,
        now_ms(),
    )
}

/// How long after an explicit send a document save still counts as part of
/// it: the editor may flush its buffer to disk just after the action fires.
pub const EXPLICIT_SEND_SAVE_GRACE_MS: u64 = 5_000;

/// The operator's explicit send (`#claimedsteerwake`): `Run Agent Doc` /
/// `agent-doc route` fired at `sent_at_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ExplicitSend {
    pub sent_at_ms: u64,
    /// Authoritative document bytes at send time. Attached-editor sends must
    /// match this hash: disk mtime cannot distinguish a later unsaved edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

impl ExplicitSend {
    /// Whether the current authoritative document is covered by this send.
    ///
    /// New records carry a content identity and therefore remain correct when
    /// an editor is ahead of disk. Detached documents retain the native-save
    /// grace because disk mtime identifies their current replica; attached
    /// documents require an exact content match.
    pub fn covers(
        &self,
        document_changed_ms: Option<u64>,
        content_hash: &str,
        editor_attached: bool,
    ) -> bool {
        if self.content_hash.as_deref() == Some(content_hash) {
            return true;
        }
        if editor_attached {
            return false;
        }
        document_changed_ms.is_none_or(|changed| {
            changed <= self.sent_at_ms.saturating_add(EXPLICIT_SEND_SAVE_GRACE_MS)
        })
    }
}

fn explicit_send_key(file: &Path) -> String {
    state_key("explicit_send", file)
}

/// Record an explicit operator send for `file` (`#claimedsteerwake`). Every
/// steering consumer (the PostToolUse hook, the turn-boundary report, the idle
/// wake) then flushes the pending steering at once, bypassing the typing gate,
/// and marks it `sent=explicit`.
pub fn record_explicit_send(file: &Path) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let content_hash = match crate::resolve_current_document_content(file, "steering_explicit_send")
    {
        Ok(content) => Some(core::content_hash(&content)),
        Err(error) => {
            eprintln!(
                "[steering] warning: could not capture authoritative content for explicit send {}: {error:#}",
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "steering_explicit_send_content_unavailable file={} error={}",
                    file.display(),
                    format!("{error:#}").replace('\n', "\\n")
                ),
            );
            None
        }
    };
    let send = ExplicitSend {
        sent_at_ms: now_ms(),
        content_hash,
    };
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &explicit_send_key(file),
        &serde_json::to_string(&send)?,
        send.sent_at_ms,
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "steering_explicit_send file={} sent_at_ms={} (#claimedsteerwake)",
            file.display(),
            send.sent_at_ms
        ),
    );
    Ok(())
}

fn load_explicit_send(
    conn: &agent_doc_sqlite::state_store::Connection,
    file: &Path,
) -> Option<ExplicitSend> {
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        conn,
        &explicit_send_key(file),
    )
    .ok()
    .flatten()
    .and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Route queue items a worker claimed to their owner (`#claimedsteerwake`) and
/// synthesize authoritative cancellation for claimed rows the operator removed
/// (`#queue-removal-cancels`).
fn route_claimed_items(
    file: &Path,
    content: &str,
    watermark: &SteeringWatermark,
    items: &mut Vec<SteeringItem>,
) -> Result<BTreeSet<String>> {
    let ledger = agent_doc_queue_io::queue_claim::load_ledger_following(file, content)
        .with_context(|| format!("read queue claims while routing {}", file.display()))?;
    let now = agent_doc_queue_io::queue_claim::claim_clock_now_secs();
    let mut attempts = ledger
        .active(now, None)
        .into_iter()
        .map(|claim| (claim.identity.clone(), claim.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    // Only a live claim owns edit forwarding. Report receipts are added below
    // as cancellation/mismatch evidence, but must continue through
    // `apply_release_receipts` to become `persist_reported`.
    if !attempts.is_empty() {
        // The steering line is the authored row. A nested leaf's claim key is
        // its nest vector, so the row text alone does not find the attempt
        // (`#nestlistlineage`).
        let _steer = agent_doc_queue_io::queue_claim::steer_guard_for(file, content);
        let row_keys = agent_doc_queue::queue_continuation::live_executable_row_keys(content)
            .unwrap_or_default();
        let claim_for = |text: &str| {
            let identity = agent_doc_queue::queue_claim::claim_identity(text);
            if let Some(claim) = attempts.get(&identity) {
                return Some(claim.clone());
            }
            let head = agent_doc_element_queue::subprompts::head_without_subprompts(text);
            let head = head.trim();
            let exact: Vec<_> = row_keys
                .iter()
                .filter(|(display, _)| display.trim() == text.trim())
                .collect();
            let candidates: Vec<_> = if !exact.is_empty() {
                exact
            } else if head.is_empty() {
                Vec::new()
            } else {
                row_keys
                    .iter()
                    .filter(|(display, _)| {
                        agent_doc_element_queue::subprompts::head_without_subprompts(display).trim()
                            == head
                    })
                    .collect()
            };
            let mut lineage_identity = None;
            for (_, lineage) in candidates {
                let id = agent_doc_queue::queue_claim::claim_identity(lineage);
                match &lineage_identity {
                    None => lineage_identity = Some(id),
                    Some(existing) if existing == &id => {}
                    Some(_) => return None,
                }
            }
            lineage_identity.and_then(|id| attempts.get(&id).cloned())
        };
        core::apply_claim_attempts(items, claim_for);
    }
    let mut cancellations = std::collections::HashMap::new();
    for receipt in &ledger.cancellations {
        cancellations
            .entry(receipt.claim.identity.clone())
            .and_modify(|claim: &mut agent_doc_queue::queue_claim::QueueClaim| {
                if receipt.claim.generation > claim.generation {
                    *claim = receipt.claim.clone();
                }
            })
            .or_insert_with(|| receipt.claim.clone());
    }
    let released = agent_doc_queue_io::claim_release_receipt::load(file)
        .with_context(|| format!("read reported receipts while routing {}", file.display()))?;
    for receipt in &released.receipts {
        if ledger.cancellations.iter().any(|cancelled| {
            cancelled.claim.identity == receipt.identity
                && cancelled.claim.generation == receipt.generation
        }) {
            continue;
        }
        attempts.entry(receipt.identity.clone()).or_insert_with(|| {
            agent_doc_queue::queue_claim::QueueClaim {
                identity: receipt.identity.clone(),
                item_text: receipt.item_text.clone(),
                owner: receipt.owner.clone(),
                generation: receipt.generation,
                state: agent_doc_queue::queue_claim::QueueClaimState::Reported,
                worker: receipt.worker_id.as_ref().map(|worker_id| {
                    agent_doc_queue::queue_claim::QueueWorkerBinding {
                        worker_id: worker_id.clone(),
                        started_at_secs: receipt.claimed_at_secs,
                    }
                }),
                dispatch_policy: None,
                tracked_item_aliases: Vec::new(),
                claimed_at_secs: receipt.claimed_at_secs,
                expires_at_secs: receipt.expires_at_secs,
                owner_session: None,
                anchor: None,
                followed_from: Vec::new(),
                followed_generation: 0,
                claim_time_row: None,
            }
        });
    }
    // A live editor can transiently publish a stale whole-buffer projection
    // while its actual operator ops touch only an unrelated queue row. The
    // projection is authoritative for text display, but it is not sufficient
    // provenance for the irreversible cancellation tombstone. In that case,
    // authorize removals only when the editor's captured operations replayed
    // onto this cycle's baseline remove the same claim identity. Detached disk
    // edits have no competing editor authority and retain the classic path.
    let operator_removed =
        if agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(file) {
            let operator_cut =
                agent_doc_op_capture_io::operator_text_for_base(file, &watermark.baseline)
                    .with_context(|| {
                        format!(
                            "read operator edit provenance while routing claimed removals for {}",
                            file.display()
                        )
                    })?;
            Some(
                operator_cut
                    .as_deref()
                    .map(|cut| removed_queue_claim_identities(&watermark.baseline, cut))
                    .unwrap_or_default(),
            )
        } else {
            None
        };
    core::append_claimed_queue_removals(items, watermark, content, |text| {
        let identity = agent_doc_queue::queue_claim::claim_identity(text);
        if operator_removed
            .as_ref()
            .is_some_and(|removed| !removed.contains(&identity))
        {
            return None;
        }
        attempts
            .get(&identity)
            .or_else(|| cancellations.get(&identity))
            .cloned()
    });
    let struck_removals =
        append_struck_removal_mismatches(file, content, watermark, &attempts, items)?;
    for item in items
        .iter()
        .filter(|item| item.dispatch == core::SteeringDispatch::CancelClaimed)
    {
        cancel_removed_claim_attempt(file, content, item)?;
    }
    // `#reportedrelease`: a line whose worker reported (released with a
    // receipt) is the coordinator's to persist now, never new dispatch work.
    let reported =
        agent_doc_queue_io::claim_release_receipt::reported_owners_for_content(file, content);
    if !reported.is_empty() {
        core::apply_release_receipts(items, |text| {
            reported
                .get(&agent_doc_queue::queue_claim::claim_identity(text))
                .cloned()
        });
    }
    Ok(struck_removals)
}

fn queue_claim_identities(
    content: &str,
) -> std::collections::HashSet<agent_doc_element_queue::QueueItemIdentity> {
    let body = agent_doc_queue::queue_prompt_drift::queue_component_text(content);
    let entries = agent_doc_queue::document_queue::parse(&body).unwrap_or_default();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            agent_doc_queue::document_queue::QueueEntry::Prompt(_) => {
                agent_doc_queue::queue_claim::claim_identity_at(&entries, index)
            }
            _ => None,
        })
        .collect()
}

fn removed_queue_claim_identities(
    before: &str,
    after: &str,
) -> std::collections::HashSet<agent_doc_element_queue::QueueItemIdentity> {
    let after = queue_claim_identities(after);
    queue_claim_identities(before)
        .into_iter()
        .filter(|identity| !after.contains(identity))
        .collect()
}

fn all_queue_item_identities(
    content: &str,
) -> std::collections::HashSet<agent_doc_element_queue::QueueItemIdentity> {
    let body = agent_doc_queue::queue_prompt_drift::queue_component_text(content);
    let entries = agent_doc_queue::document_queue::parse(&body).unwrap_or_default();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            agent_doc_queue::document_queue::QueueEntry::Prompt(_)
            | agent_doc_queue::document_queue::QueueEntry::Completed(_) => {
                agent_doc_queue::queue_claim::claim_identity_at(&entries, index)
            }
            _ => None,
        })
        .collect()
}

/// Struck rows of `content` with the identity their queue position names.
fn completed_queue_identities(
    content: &str,
) -> Vec<(agent_doc_element_queue::QueueItemIdentity, String)> {
    let body = agent_doc_queue::queue_prompt_drift::queue_component_text(content);
    let entries = agent_doc_queue::document_queue::parse(&body).unwrap_or_default();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            agent_doc_queue::document_queue::QueueEntry::Completed(prompt) => {
                let identity = agent_doc_queue::queue_claim::claim_identity_at(&entries, index)?;
                Some((identity, prompt.text.clone()))
            }
            _ => None,
        })
        .collect()
}

/// A struck-row deletion is presentation cleanup only when durable state also
/// proves terminality. Conflicting claim or reported-receipt evidence is
/// surfaced as degraded and is never converted into cancellation authority.
///
/// `#deletedstruckmismatch`: a binary completion cell is a transient projection
/// of a claim attempt, never ownership on its own. Once the struck row's claim
/// closed and no reported receipt remains, a leftover cell (in the document
/// history or a retained outbox target) is stale residue that
/// `queue repair-stale-completion` retires; the binary-owned prune of that row
/// is cleanup, not an operator deletion of owned work. A mismatch is reported
/// once per owning attempt per consumer: the returned keys join the consumer's
/// watermark, and a key already there is never re-reported.
fn append_struck_removal_mismatches(
    file: &Path,
    content: &str,
    watermark: &SteeringWatermark,
    attempts: &std::collections::HashMap<
        agent_doc_element_queue::QueueItemIdentity,
        agent_doc_queue::queue_claim::QueueClaim,
    >,
    items: &mut Vec<SteeringItem>,
) -> Result<BTreeSet<String>> {
    let mut reported = BTreeSet::new();
    let current = all_queue_item_identities(content);
    let deleted = completed_queue_identities(&watermark.baseline)
        .into_iter()
        .filter(|(identity, _)| !current.contains(identity))
        .collect::<Vec<_>>();
    if deleted.is_empty() {
        return Ok(reported);
    }
    let receipts = agent_doc_queue_io::claim_release_receipt::load(file)
        .with_context(|| format!("read reported receipts while routing {}", file.display()))?;

    for (identity, text) in deleted {
        let claim = attempts.get(&identity);
        let receipt = receipts
            .receipts
            .iter()
            .find(|receipt| receipt.identity == identity);
        if claim.is_none() && receipt.is_none() {
            continue;
        }
        let owner = claim
            .map(|claim| claim.owner.clone())
            .or_else(|| receipt.map(|receipt| receipt.owner.clone()));
        let generation = claim
            .map(|claim| claim.generation)
            .or_else(|| receipt.map(|receipt| receipt.generation));
        let key = core::struck_removal_report_key(&identity, owner.as_deref(), generation);
        if watermark.surfaced_struck_removals.contains(&key) || !reported.insert(key) {
            continue;
        }
        items.push(SteeringItem {
            source: core::SteeringSource::Queue,
            change: core::SteeringChange::Deleted,
            dispatch: core::SteeringDispatch::DegradedClaimMismatch,
            current_item: false,
            verbatim: text,
            previous: None,
            presets: Vec::new(),
            possibly_partial: false,
            owner,
            generation,
            worker_id: claim
                .and_then(|claim| claim.worker.as_ref().map(|worker| worker.worker_id.clone()))
                .or_else(|| receipt.and_then(|receipt| receipt.worker_id.clone())),
            explicit: false,
        });
    }
    Ok(reported)
}

/// Serialize removal against `queue report-complete`, tombstone the exact
/// claim generation, and publish a completion-cell retirement successor. The
/// shared lock closes the report-marked/cell-not-yet-retained race; the
/// retained `queue_consume` composition closes the delayed-outbox replay race.
fn cancel_removed_claim_attempt(file: &Path, content: &str, item: &SteeringItem) -> Result<()> {
    let lock_path = agent_doc_queue_io::queue_claim::completion_feedback_lock_path(file)?;
    let lock = agent_doc_sync_io::acquire_sync_lock(&lock_path, Duration::from_secs(5), |_| {});
    let agent_doc_sync_io::SyncLockAcquire::Acquired(_lock) = lock else {
        anyhow::bail!(
            "timed out waiting to tombstone removed queue claim for {}; no cancellation state was changed",
            file.display()
        );
    };
    let receipt = match agent_doc_queue_io::queue_claim::cancel_removed_attempt(
        file,
        &item.verbatim,
        item.generation,
    ) {
        Ok(receipt) => receipt,
        Err(err)
            if matches!(
                err.downcast_ref::<agent_doc_queue::queue_claim::QueueClaimTransitionRefused>(),
                Some(agent_doc_queue::queue_claim::QueueClaimTransitionRefused::Missing)
            ) =>
        {
            let identity = agent_doc_queue::queue_claim::claim_identity(&item.verbatim);
            let reported = agent_doc_queue_io::claim_release_receipt::load(file)?
                .receipts
                .into_iter()
                .find(|receipt| {
                    receipt.identity == identity
                        && (receipt.generation == 0
                            || item.generation == Some(receipt.generation))
                })
                .with_context(|| {
                format!(
                    "removed queue item {:?} no longer has its generation-fenced claim or reported receipt",
                    item.verbatim
                )
            })?;
            let cancellation =
                agent_doc_queue_io::queue_claim::record_released_cancellation(file, &reported)?;
            agent_doc_queue_io::claim_release_receipt::cancel_reported_attempt(
                file,
                &item.verbatim,
                item.generation,
            )?;
            cancellation
        }
        Err(err) => return Err(err),
    };
    let Some(completion_key) = receipt.completion_key.as_deref() else {
        return Ok(());
    };
    let retired = agent_doc_document::subagent_completion::strip_completion_cells_matching(
        content,
        |evidence| evidence.key == completion_key,
    );
    let pending = agent_doc_document_realtime_io::pending_document_write(file).is_some();
    if !pending && retired == content {
        return Ok(());
    }
    agent_doc_document_realtime_io::retain_deferred_document_write_target(
        file,
        content,
        &retired,
        "queue_consume",
        agent_doc_document_realtime_io::DocumentWriteDeferredReason::EditorProjectionPending,
    )?;
    let editor_attached = agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached();
    if !editor_attached {
        agent_doc_document_realtime_io::materialize_last_editor_close_through_authority(
            file,
            "queue_cancel_completion_tombstone",
        )?;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_cancel_completion_tombstone owner={} worker={} generation={} completion_key={} pending_outbox={} delivery=retired_successor (#queue-removal-cancels)",
            receipt.claim.owner,
            receipt
                .claim
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str())
                .unwrap_or("-"),
            receipt.claim.generation,
            completion_key,
            pending,
        ),
    );
    Ok(())
}

fn load_watermark(
    conn: &agent_doc_sqlite::state_store::Connection,
    key: &str,
) -> Result<Option<SteeringWatermark>> {
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(conn, key)?
        .map(|raw| serde_json::from_str(&raw).context("parse mid-turn steering watermark"))
        .transpose()
}

fn stat_fingerprint(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{mtime}:{}", meta.len())
}

fn mtime_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

/// Resolve the debounce window: frontmatter, then project config, then default.
pub fn debounce_ms_for(file: &Path, content: &str) -> u64 {
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.steering_debounce_ms);
    resolve_debounce_ms(
        frontmatter,
        agent_doc_project_config_io::load_project_for_doc(file).agent_doc_steering_debounce_ms,
    )
}

/// The steering max-hold (`#steeringtypinggate`): project config, else the
/// shared default.
pub fn max_hold_ms_for(file: &Path) -> u64 {
    agent_doc_project_config_io::load_project_for_doc(file)
        .agent_doc_steering_max_hold_ms
        .unwrap_or(agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS)
}

/// Pure precedence for [`debounce_ms_for`].
pub fn resolve_debounce_ms(frontmatter: Option<u64>, project: Option<u64>) -> u64 {
    frontmatter
        .or(project)
        .unwrap_or(DEFAULT_STEERING_DEBOUNCE_MS)
}

/// Whether the cycle still accepts mid-turn steering: the same cycle, open,
/// and no response captured yet. After capture, closeout `session-check` owns
/// steering (`realtime_steering_closeout_guidance`).
fn cycle_accepts_steering(cycle: &agent_doc_cycle_state_io::CycleState, cycle_id: &str) -> bool {
    cycle.cycle_id == cycle_id
        && matches!(cycle.phase, agent_doc_turn::CyclePhase::PreflightStarted)
        && cycle.capture_id.is_none()
        && cycle.response_sha256.is_none()
}

fn binary_owned_ids(cycle: &agent_doc_cycle_state_io::CycleState) -> BTreeSet<String> {
    cycle
        .pending_actionable_ids
        .iter()
        .chain(cycle.pending_added_ids.iter())
        .chain(cycle.requested_added_ids.iter())
        .map(|id| id.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect()
}

/// A computed, not-yet-persisted observation for one consumer.
///
/// Splitting compute from persist is what lets the turn-boundary report
/// advance the watermark only after the report actually carried the items.
#[derive(Debug, Clone)]
pub struct PreparedObservation {
    consumer: String,
    consumer_key: String,
    root: PathBuf,
    next: Option<SteeringWatermark>,
    /// The report, `None` when there is nothing to say.
    pub report: Option<SteeringReport>,
    /// The observation happened at/after the turn boundary.
    pub boundary: bool,
    /// The settle decisions behind the report, for the completion-gate
    /// decision log (`#steergatelog`); recorded only when acknowledged.
    gate: Option<crate::steering_gate_log::GateObservation>,
    max_hold_ms: u64,
}

impl PreparedObservation {
    /// Persist the advanced watermark (and log what surfaced).
    pub fn acknowledge(&self, file: &Path) -> Result<()> {
        let Some(next) = &self.next else {
            return Ok(());
        };
        let conn = agent_doc_sqlite::state_store::open_state_db(&self.root)?;
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            &self.consumer_key,
            &serde_json::to_string(next)?,
            now_ms(),
        )?;
        if let Some(report) = self
            .report
            .as_ref()
            .filter(|report| !report.items.is_empty())
        {
            // `#steerbaselineabsorb`: a turn-boundary report frames each
            // exchange item as the NEXT cycle's prompt. When the closeout commit
            // already absorbed its text, the next preflight diffs clean, so
            // persist it for that contract to carry.
            if self.boundary {
                crate::absorbed_steering::record_reported_logged(
                    file,
                    &report.items,
                    &next.cycle_id,
                );
            }
            let dispatches = report
                .items
                .iter()
                .map(|item| item.dispatch.as_str())
                .collect::<Vec<_>>()
                .join(",");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "midturn_steering_surfaced file={} consumer={} cycle={} count={} pending={} dispatch={dispatches} boundary={}",
                    file.display(),
                    self.consumer,
                    next.cycle_id,
                    report.items.len(),
                    report.pending,
                    self.boundary,
                ),
            );
        }
        // `#steergatelog`: the decisions this consumer just acted on feed the
        // decision log's graph; its Effect persists them. A log failure never
        // fails steering delivery.
        if let Some(gate) = &self.gate
            && let Err(err) =
                crate::steering_gate_log::record(&self.root, file, gate.clone(), self.max_hold_ms)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "steering_gate_log_failed file={} consumer={} error={err:#} (#steergatelog)",
                    file.display(),
                    self.consumer,
                ),
            );
        }
        Ok(())
    }
}

/// How an observation treats a cycle that no longer accepts mid-turn steering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClosedCyclePolicy {
    /// The in-turn hook. While the cycle is open it reports in-turn
    /// steering; once the cycle closes it keeps reporting unsurfaced changes in
    /// boundary mode (`#steeringafterclose`). An agent that keeps working in
    /// the same harness turn after closeout (dispatching subagents, waiting on
    /// them) used to go deaf here: the 2026-10-03 `#subagent` additions to
    /// agent-doc-bugs.md landed while tool calls were still running and the
    /// silenced hook dropped them.
    Hook,
    /// Polls (`agent-doc steering`): after close, keep reporting unsurfaced
    /// queue/exchange changes in boundary mode until the next preflight.
    Boundary,
    /// The turn-boundary report itself: always boundary mode, no debounce.
    ForceBoundary,
}

/// Fallback base when no seeded watermark describes the latest cycle: the
/// last committed baseline, so a poll after close still reports items added
/// since the last commit.
fn committed_baseline_watermark(file: &Path) -> Result<Option<SteeringWatermark>> {
    Ok(
        agent_doc_snapshot_io::load_document_baseline(file)?.map(|baseline| {
            let _nested_claim_steer =
                agent_doc_queue_io::queue_claim::steer_guard_for(file, &baseline);
            SteeringWatermark::seed(
                &format!("committed:{}", core::content_hash(&baseline)),
                &baseline,
                None,
                Vec::new(),
            )
        }),
    )
}

fn prepare(
    file: &Path,
    consumer: &str,
    policy: ClosedCyclePolicy,
) -> Result<Option<PreparedObservation>> {
    prepare_with_gate(file, consumer, policy, true, None)
}

/// [`prepare`] with the unchanged-document gate optional. The gate is a pure
/// optimization for advancing consumers; a non-advancing reader that needs the
/// full unsurfaced set relative to a watermark (the idle wake) turns it off.
fn prepare_with_gate(
    file: &Path,
    consumer: &str,
    policy: ClosedCyclePolicy,
    unchanged_gate: bool,
    carried: Option<&WakeSettleCarry>,
) -> Result<Option<PreparedObservation>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let stored_base = load_watermark(&conn, &state_key("base", file))?;
    // Boundary-capable consumers need the cycle up front to pick the base;
    // the hook keeps its rare-path cycle lookup.
    let cycle = if policy == ClosedCyclePolicy::Hook {
        None
    } else {
        agent_doc_cycle_state_io::load_with_closeout_projection(file)?
    };
    // The boundary report speaks for a committed turn; a closeout that
    // deferred (cycle still open) reports at the later session-check.
    if policy == ClosedCyclePolicy::ForceBoundary && cycle.as_ref().is_some_and(|c| c.is_open()) {
        return Ok(None);
    }
    let base_describes_cycle = |base: &SteeringWatermark| {
        cycle
            .as_ref()
            .is_none_or(|cycle| cycle.cycle_id == base.cycle_id)
    };
    let base = match stored_base {
        Some(base) if policy == ClosedCyclePolicy::Hook || base_describes_cycle(&base) => base,
        // No seed for the latest cycle (or none at all): compare with the
        // last committed baseline.
        _ if policy != ClosedCyclePolicy::Hook => match committed_baseline_watermark(file)? {
            Some(base) => base,
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    let consumer_key = state_key(consumer, file);
    let mut watermark = match load_watermark(&conn, &consumer_key)? {
        Some(existing)
            if existing.cycle_id == base.cycle_id && existing.baseline == base.baseline =>
        {
            existing
        }
        _ => base,
    };
    // `#fixthrashingbackground`: a non-advancing reader (the idle wake) never
    // writes its watermark, so the settle gate would see every held candidate
    // for the first time on every observation. Its in-process carry supplies the
    // `first_seen_ms` the durable watermark cannot.
    if let Some(carried) = carried {
        carried.merge_into(&mut watermark);
    }
    let cycle_open = cycle
        .as_ref()
        .is_some_and(|cycle| cycle_accepts_steering(cycle, &watermark.cycle_id));
    let mut boundary = match policy {
        // A watermark a pre-`#steeringafterclose` hook silenced already knows
        // its cycle closed.
        ClosedCyclePolicy::Hook => watermark.closed,
        ClosedCyclePolicy::Boundary => !cycle_open,
        ClosedCyclePolicy::ForceBoundary => true,
    };
    // A silenced watermark still owes the boundary report.
    watermark.closed = false;
    let max_hold_ms = max_hold_ms_for(file);
    let prepared = |next: Option<SteeringWatermark>,
                    report: Option<SteeringReport>,
                    boundary: bool,
                    gate: Option<crate::steering_gate_log::GateObservation>| {
        Ok(Some(PreparedObservation {
            consumer: consumer.to_string(),
            consumer_key: consumer_key.clone(),
            root: root.clone(),
            next,
            report,
            boundary,
            gate,
            max_hold_ms,
        }))
    };

    // Hot-path gate for detached documents: unchanged file stat and nothing
    // settling → no read. An attached editor can change authoritative text
    // without changing disk metadata, so it must resolve the CRDT projection
    // before applying the content-hash gate below (`#gh173`).
    let meta = std::fs::metadata(file).with_context(|| format!("stat {}", file.display()))?;
    let fingerprint = stat_fingerprint(&meta);
    let editor_attached = agent_doc_crdt_relay_io::crdt_authority_for_file(file).editor_attached();
    if unchanged_gate
        && watermark.pending.is_empty()
        && watermark.last_observed_stat.as_deref() == Some(fingerprint.as_str())
        && !editor_attached
    {
        return prepared(None, None, boundary, None);
    }
    let content = crate::resolve_current_document_content(file, "midturn_steering")?;
    let _nested_claim_steer = agent_doc_queue_io::queue_claim::steer_guard_for(file, &content);
    let content_hash = core::content_hash(&content);
    let selected_head_open = core::current_item_is_present(&watermark, &content);
    // `#openheadsteering`: stale-lock repair can terminalize the cycle record
    // while the selected queue head and its owning harness turn are still live.
    // Queue lifecycle evidence wins over that stale terminal projection. A
    // terminal report must stay silent; ordinary consumers keep in-turn wording.
    if selected_head_open && boundary {
        if policy == ClosedCyclePolicy::ForceBoundary {
            return Ok(None);
        }
        boundary = false;
    }
    if unchanged_gate
        && watermark.pending.is_empty()
        && watermark.last_observed_content_hash.as_deref() == Some(content_hash.as_str())
    {
        watermark.last_observed_stat = Some(fingerprint);
        return prepared(Some(watermark), None, boundary, None);
    }

    let disk_changed_ms = mtime_ms(&meta);
    let explicit_send = load_explicit_send(&conn, file)
        .is_some_and(|send| send.covers(disk_changed_ms, &content_hash, editor_attached));
    // The turn boundary is the last chance before the loop re-enters: an
    // item that is complete surfaces now rather than waiting out the window.
    let debounce_ms = if policy == ClosedCyclePolicy::ForceBoundary || explicit_send {
        0
    } else {
        debounce_ms_for(file, &content)
    };
    let mode = if boundary {
        core::ObserveMode::Boundary
    } else {
        core::ObserveMode::InTurn
    };
    let empty = BTreeSet::new();
    let owned = cycle.as_ref().map(binary_owned_ids).unwrap_or_default();
    let observed_at_ms = now_ms();
    // The relay currently exposes content identity, not an editor edit
    // timestamp. `None` makes the settle gate use per-item stability instead of
    // pretending the stale disk mtime is the live edit time (`#gh173`).
    let document_changed_ms = if editor_attached {
        None
    } else {
        disk_changed_ms
    };
    let document = crate::steering_gate_log::document_key(&root, file);
    // `#steergateperceptron`: the online-learned gate answers first when
    // enabled; the deterministic floors stay inside the settle decision, and
    // the gate lives in the binary, so every harness gets the same answer.
    let learned = crate::steering_gate_log::learned_classifier(
        &root,
        file,
        &document,
        observed_at_ms,
        max_hold_ms,
    );
    let deterministic = agent_doc_debounce::edit_settle::DeterministicOnly;
    let classifier: &dyn agent_doc_debounce::edit_settle::CompletionClassifier = match &learned {
        Some(gate) => gate,
        None => &deterministic,
    };
    let ctx = ObserveContext {
        now_ms: observed_at_ms,
        document_changed_ms,
        debounce_ms,
        binary_owned_queue_ids: if boundary { &owned } else { &empty },
        explicit_send,
        max_hold_ms,
        classifier,
        median_pause_ms: crate::steering_gate_log::median_pause_ms(
            &root,
            file,
            &document,
            observed_at_ms,
            max_hold_ms,
        ),
    };
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(&content, file)?;
    let mut observation =
        core::observe_with_mode_and_presets(&watermark, &content, &ctx, mode, &prompt_presets);
    if !boundary && !observation.ready.is_empty() {
        // Rare path: confirm the cycle is still open before handing anything
        // to the agent, and exclude this cycle's own queue bookkeeping.
        let cycle = match cycle {
            Some(cycle) => Some(cycle),
            None => agent_doc_cycle_state_io::load_with_closeout_projection(file)?,
        };
        match cycle {
            Some(cycle) if cycle_accepts_steering(&cycle, &watermark.cycle_id) => {
                let owned = binary_owned_ids(&cycle);
                if !owned.is_empty() {
                    observation = core::observe_with_mode_and_presets(
                        &watermark,
                        &content,
                        &ObserveContext {
                            binary_owned_queue_ids: &owned,
                            ..ctx
                        },
                        core::ObserveMode::InTurn,
                        &prompt_presets,
                    );
                }
            }
            closed if !core::current_item_is_present(&watermark, &content) => {
                // The cycle closed while this harness turn kept running:
                // report the same edits in boundary terms (queue work for the
                // next cycle), excluding the closed cycle's own bookkeeping.
                boundary = true;
                let owned = closed.as_ref().map(binary_owned_ids).unwrap_or_default();
                observation = core::observe_with_mode_and_presets(
                    &watermark,
                    &content,
                    &ObserveContext {
                        binary_owned_queue_ids: &owned,
                        ..ctx
                    },
                    core::ObserveMode::Boundary,
                    &prompt_presets,
                );
            }
            Some(_) => {
                // A stale terminal cycle record does not close a queue head
                // that is visibly still selected in the authoritative text.
            }
            None => {
                // A seed without any surviving cycle record has no live turn
                // whose head can outrank the boundary. Preserve the legacy
                // committed-baseline fallback for polling-only consumers.
                boundary = true;
                observation = core::observe_with_mode_and_presets(
                    &watermark,
                    &content,
                    &ctx,
                    core::ObserveMode::Boundary,
                    &prompt_presets,
                );
            }
        }
    }
    if boundary {
        // `#assistantsteeringlineage`: the boundary diff starts at preflight so
        // operator edits absorbed by closeout survive. Subtract only the exact
        // response body attested by this cycle's capture lineage. If that
        // lineage is incomplete, fail without advancing the watermark; agent
        // prose must never be dispatched as operator intent.
        crate::absorbed_steering::retain_operator_lineage(
            file,
            &watermark.cycle_id,
            &mut observation.ready,
        )?;
    }
    let gate = crate::steering_gate_log::GateObservation {
        document,
        consumer: consumer.to_string(),
        harness: document_harness(&content),
        operator: crate::steering_gate_log::operator_id(),
        now_ms: ctx.now_ms,
        document_changed_ms: ctx.document_changed_ms,
        boundary,
        learned_gate: learned.is_some(),
        explicit: explicit_send,
        decisions: std::mem::take(&mut observation.decisions),
    };
    let struck_removals = route_claimed_items(file, &content, &watermark, &mut observation.ready)?;
    // `#steerbaselineabsorb`: a prompt a cycle already carried or answered is
    // never re-surfaced (the watermark still advances past it).
    crate::absorbed_steering::drop_consumed(file, &mut observation.ready);
    let mut next = observation.next;
    // `#deletedstruckmismatch`: a degraded struck-row deletion is delivered
    // once; acknowledging this observation records it.
    next.surfaced_struck_removals.extend(struck_removals);
    next.last_observed_stat = Some(fingerprint);
    let report = SteeringReport {
        document: file.display().to_string(),
        cycle_id: next.cycle_id.clone(),
        items: observation.ready,
        pending: observation.pending,
        after_close: boundary,
        recheck_after_ms: observation.recheck_after_ms,
    };
    prepared(Some(next), Some(report), boundary, Some(gate))
}

/// The document's configured harness (frontmatter `agent:`), for the
/// decision log. The gate itself never reads it: decisions are identical
/// across harnesses.
fn document_harness(content: &str) -> String {
    agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.agent)
        .map(|agent| agent.trim().to_ascii_lowercase())
        .filter(|agent| !agent.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Observe `file` for `consumer`. Returns `Ok(None)` when there is no active
/// cycle watermark, the cycle closed (hook only), or the document is
/// unchanged; otherwise a report (possibly with zero ready items while edits
/// are still settling).
///
/// Every consumer keeps reporting unsurfaced changes after the cycle closes,
/// in boundary mode, until the next preflight re-seeds (`#closeout-steering`,
/// `#steeringafterclose`).
///
/// When `advance` is false the watermark is not written (a peek).
pub fn observe(file: &Path, consumer: &str, advance: bool) -> Result<Option<SteeringReport>> {
    let policy = if consumer == CONSUMER_HOOK {
        ClosedCyclePolicy::Hook
    } else {
        ClosedCyclePolicy::Boundary
    };
    let Some(prepared) = prepare(file, consumer, policy)? else {
        return Ok(None);
    };
    if advance {
        prepared.acknowledge(file)?;
    }
    Ok(prepared.report)
}

/// The turn-boundary observation for the in-turn agent channel: unsurfaced
/// steering relative to the hook consumer's watermark (so nothing the hook
/// already delivered repeats), in boundary mode, with no debounce. The caller
/// renders it, emits it, and only then calls
/// [`PreparedObservation::acknowledge`].
pub fn prepare_closeout(file: &Path) -> Result<Option<PreparedObservation>> {
    prepare(file, CONSUMER_HOOK, ClosedCyclePolicy::ForceBoundary)
}

/// What the idle supervisor's steering wake sees (`#steeringwake`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WakeObservation {
    /// Settled steering no agent-facing consumer has surfaced and no worker
    /// has claimed, in document order.
    pub items: Vec<SteeringItem>,
    /// Identity of the whole set: the steering base's cycle plus every item.
    /// Empty when `items` is empty.
    pub fingerprint: String,
    /// Candidates still held by the settle gate.
    pub pending: usize,
    /// When a held candidate can next settle, in ms from now. The supervisor
    /// schedules exactly one re-observation for it instead of polling.
    pub recheck_after_ms: Option<u64>,
}

fn steering_item_identity(item: &SteeringItem) -> String {
    format!(
        "{}:{}:{}",
        match item.source {
            core::SteeringSource::Exchange => "exchange",
            core::SteeringSource::Queue => "queue",
        },
        match item.change {
            core::SteeringChange::Added => "added",
            core::SteeringChange::Edited => "edited",
            core::SteeringChange::Deleted => "deleted",
        },
        core::normalize_queue_text(&item.verbatim)
    )
}

/// Non-consuming observation for the idle steering wake (`#steeringwake`).
///
/// The same derivation every steering consumer uses ([`prepare`]), read
/// against the agent channel's watermark (the hook consumer, which the
/// turn-boundary report also advances) without advancing it. The `steering`
/// poll watermark is deliberately NOT consulted: a consuming diagnostic read
/// (`agent-doc steering` run by an operator or another agent) must never
/// suppress the wake. The cost is at most one duplicate for a hookless
/// harness that polled an item after closeout; the alternative is loss.
/// Queue items a worker has
/// claimed (`agent-doc queue claim`) are in flight elsewhere and never wake
/// the session. The woken turn receives the items through its normal
/// channels (preflight, the hook, or the boundary report), so the wake itself
/// never consumes steering.
pub fn observe_for_wake(file: &Path) -> Result<WakeObservation> {
    observe_for_wake_with(file, &mut WakeSettleCarry::default())
}

/// Settle evidence the idle steering wake carries between its own observations
/// (`#fixthrashingbackground`).
///
/// The wake reads the hook consumer's watermark without advancing it (it must
/// never consume steering), so the durable watermark never learns when the
/// wake first saw a held candidate. With a live editor attached the document
/// has no edit timestamp either (`document_changed_ms = None`), so the settle
/// gate had no evidence that time had passed at all: every candidate was
/// "first seen now", stayed `Held`, and asked for another re-observation —
/// forever. Each one re-read the full document text through the controller.
/// Observed live 2026-10-12 (haiven-dev): ~30 idle supervisors re-reading
/// every open document roughly every 17 seconds, 6897 of the last 9577
/// `controller_crdt_current_text` reads.
///
/// The carry is plain in-process data the supervisor owns for its lifetime; it
/// is keyed by the watermark's cycle and baseline so a re-seed starts fresh. A
/// supervisor restart forgets it, which costs at most one settle window.
/// Upper bound on the candidates one [`WakeSettleCarry`] remembers.
const WAKE_SETTLE_CARRY_MAX: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WakeSettleCarry {
    identity: Option<(String, String)>,
    pending: BTreeMap<String, core::PendingObservation>,
}

impl WakeSettleCarry {
    /// Candidates the carry holds settle evidence for.
    pub fn tracked(&self) -> usize {
        self.pending.len()
    }

    fn identity_of(watermark: &SteeringWatermark) -> (String, String) {
        (
            watermark.cycle_id.clone(),
            core::content_hash(&watermark.baseline),
        )
    }

    fn merge_into(&self, watermark: &mut SteeringWatermark) {
        if self.identity.as_ref() != Some(&Self::identity_of(watermark)) {
            return;
        }
        for (key, carried) in &self.pending {
            watermark
                .pending
                .entry(key.clone())
                .and_modify(|durable| {
                    // The same version seen earlier by the wake is older
                    // evidence than a later hook observation of it.
                    if durable.content_hash == carried.content_hash
                        && carried.first_seen_ms < durable.first_seen_ms
                    {
                        *durable = carried.clone();
                    }
                })
                .or_insert_with(|| carried.clone());
        }
    }

    fn remember(&mut self, next: Option<&SteeringWatermark>) {
        let Some(next) = next else {
            *self = Self::default();
            return;
        };
        let identity = Self::identity_of(next);
        if self.identity.as_ref() != Some(&identity) {
            *self = Self {
                identity: Some(identity),
                pending: BTreeMap::new(),
            };
        }
        // A candidate that settled leaves `next.pending`, but the wake never
        // advances the durable watermark, so the next observation meets it
        // again. Keep its evidence, or it would be "first seen" — and held —
        // all over again. Bounded by the candidates of one watermark.
        for (key, observation) in &next.pending {
            self.pending
                .entry(key.clone())
                .and_modify(|kept| {
                    if kept.content_hash != observation.content_hash {
                        *kept = observation.clone();
                    }
                })
                .or_insert_with(|| observation.clone());
        }
        if self.pending.len() > WAKE_SETTLE_CARRY_MAX {
            let mut by_age: Vec<(u64, String)> = self
                .pending
                .iter()
                .map(|(key, observation)| (observation.first_seen_ms, key.clone()))
                .collect();
            by_age.sort();
            for (_, key) in by_age
                .into_iter()
                .take(self.pending.len() - WAKE_SETTLE_CARRY_MAX)
            {
                self.pending.remove(&key);
            }
        }
    }
}

/// [`observe_for_wake`] with the supervisor's [`WakeSettleCarry`], so a held
/// candidate settles (or reaches max hold) instead of being re-observed forever.
pub fn observe_for_wake_with(file: &Path, carry: &mut WakeSettleCarry) -> Result<WakeObservation> {
    let hook = prepare_with_gate(
        file,
        CONSUMER_HOOK,
        ClosedCyclePolicy::Boundary,
        false,
        Some(carry),
    )?;
    carry.remember(hook.as_ref().and_then(|prepared| prepared.next.as_ref()));
    let Some(hook_report) = hook.as_ref().and_then(|prepared| prepared.report.clone()) else {
        return Ok(WakeObservation::default());
    };
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(file, &content);
    // A claimed item is in flight elsewhere, so its ADDITION never wakes the
    // session, nor does a tag/marker-only re-tag (same work identity,
    // `#claimdispatchidentity`). An edit of its substance wakes the
    // coordinator, which forwards it to the owner (`#claimedsteerwake`).
    // That includes a nested-body edit whose parent line is unchanged
    // (`#nestedclaimsteer`): identity stays, the normalized text changes, and
    // `head_without_subprompts` does not.
    let forwards_substance = |item: &SteeringItem| {
        if item.dispatch != core::SteeringDispatch::ForwardToOwner
            || item.change != core::SteeringChange::Edited
        {
            return false;
        }
        let Some(previous) = item.previous.as_deref() else {
            return false;
        };
        if agent_doc_queue::queue_claim::claim_identity(previous)
            != agent_doc_queue::queue_claim::claim_identity(&item.verbatim)
        {
            return true;
        }
        let previous_norm = core::normalize_queue_text(previous);
        let verbatim_norm = core::normalize_queue_text(&item.verbatim);
        previous_norm != verbatim_norm
            && agent_doc_element_queue::subprompts::head_without_subprompts(&previous_norm)
                == agent_doc_element_queue::subprompts::head_without_subprompts(&verbatim_norm)
    };
    let items: Vec<SteeringItem> = hook_report
        .items
        .iter()
        .filter(|item| {
            item.source != core::SteeringSource::Queue
                || forwards_substance(item)
                || !claimed.claims(&item.verbatim)
        })
        .cloned()
        .collect();
    let fingerprint = if items.is_empty() {
        String::new()
    } else {
        let mut basis = hook_report.cycle_id.clone();
        for item in &items {
            basis.push('\n');
            basis.push_str(&steering_item_identity(item));
        }
        core::content_hash(&basis)
    };
    let pending = hook_report.pending;
    let recheck_after_ms = hook_report.recheck_after_ms;
    Ok(WakeObservation {
        items,
        fingerprint,
        pending,
        recheck_after_ms,
    })
}

/// Steering an explicit send (`#claimedsteerwake`) handed to the owning
/// turn: the settled-or-not items the agent channel has not surfaced yet,
/// read without consuming (the owning turn's hook, its boundary report, or
/// the idle wake delivers them, `sent=explicit`).
pub fn explicit_send_pending_items(file: &Path) -> Result<Vec<SteeringItem>> {
    Ok(prepare_with_gate(
        file,
        CONSUMER_HOOK,
        ClosedCyclePolicy::Boundary,
        false,
        None,
    )?
    .and_then(|prepared| prepared.report)
    .map(|report| report.items)
    .unwrap_or_default())
}

/// Document text last handed to a running owner as an explicit-send diff
/// (`#routesyncsteer`). Keyed by the cycle so a new turn does not diff
/// against the previous turn's delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
struct RouteSyncBaseline {
    cycle_id: String,
    content: String,
}

/// Steering instructions for one explicit send into a running turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSyncSteering {
    pub instructions: String,
    /// Classified explicit-send items. Zero when the payload is only the diff.
    pub pending_items: usize,
    pub cycle_id: String,
    /// Authoritative document to record after the pane accepts the paste.
    pub content: String,
}

fn route_sync_baseline_key(file: &Path) -> String {
    state_key("route_sync_baseline", file)
}

fn load_route_sync_baseline(
    conn: &agent_doc_sqlite::state_store::Connection,
    file: &Path,
) -> Option<RouteSyncBaseline> {
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        conn,
        &route_sync_baseline_key(file),
    )
    .ok()
    .flatten()
    .and_then(|raw| serde_json::from_str(&raw).ok())
}

/// The latest diff since the last successful explicit send, rendered as
/// steering instructions (`#routesyncsteer`).
///
/// The previous text is the document recorded after the last delivery in this
/// cycle, or the cycle's steering baseline when this is the first send. `None`
/// when that diff is empty and no explicit-send item is waiting.
pub fn prepare_route_sync_steering(file: &Path) -> Result<Option<RouteSyncSteering>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let content = crate::resolve_current_document_content(file, "route_sync_steering")?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let watermark = load_watermark(&conn, &state_key("base", file))?;
    let stored = load_route_sync_baseline(&conn, file);
    let items = match explicit_send_pending_items(file) {
        Ok(items) => items,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_sync_steering_items_unreadable file={} error={} (#routesyncsteer)",
                    file.display(),
                    first_error_line(&format!("{err:#}"))
                ),
            );
            Vec::new()
        }
    };
    let (previous, cycle_id) = if let Some(watermark) = watermark {
        let previous = match &stored {
            Some(stored) if stored.cycle_id == watermark.cycle_id => stored.content.clone(),
            _ => watermark.baseline.clone(),
        };
        (Some(previous), watermark.cycle_id)
    } else {
        (None, String::new())
    };
    let diff = previous
        .as_deref()
        .and_then(|previous| agent_doc_diff::unified_diff_from_contents(previous, &content));
    let instructions =
        core::render_route_sync_steering(&file.display().to_string(), diff.as_deref(), &items);
    let Some(instructions) = instructions else {
        return Ok(None);
    };
    Ok(Some(RouteSyncSteering {
        instructions,
        pending_items: items.len(),
        cycle_id,
        content,
    }))
}

fn first_error_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or(text)
        .replace('\n', " ")
        .chars()
        .take(240)
        .collect()
}

/// Remember `content` as the last document handed to the running owner, and
/// acknowledge every steering consumer so the same diff is not sent again.
pub fn record_route_sync_steering_delivered(
    file: &Path,
    cycle_id: &str,
    content: &str,
) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let baseline = RouteSyncBaseline {
        cycle_id: cycle_id.to_string(),
        content: content.to_string(),
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &route_sync_baseline_key(file),
        &serde_json::to_string(&baseline)?,
        now_ms(),
    )?;
    for consumer in [CONSUMER_HOOK, CONSUMER_CLI, CONSUMER_FOLLOW] {
        if let Err(err) = observe(file, consumer, true) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_sync_steering_ack_failed file={} consumer={consumer} error={} (#routesyncsteer)",
                    file.display(),
                    first_error_line(&format!("{err:#}"))
                ),
            );
        }
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_sync_steering_baseline_recorded file={} cycle={cycle_id} bytes={} (#routesyncsteer)",
            file.display(),
            content.len()
        ),
    );
    Ok(())
}

fn wake_receipt_key(file: &Path) -> String {
    state_key("wake", file)
}

/// The durable receipt of the last steering set a wake was delivered for.
pub fn load_wake_receipt(file: &Path) -> Result<Option<String>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &conn,
        &wake_receipt_key(file),
    )
}

/// Record that a wake for `fingerprint` was submitted to the owning pane.
pub fn record_wake_receipt(file: &Path, fingerprint: &str, items: usize) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &wake_receipt_key(file),
        fingerprint,
        now_ms(),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "steering_wake_delivered file={} fingerprint={} items={items} (#steeringwake)",
            file.display(),
            fingerprint.get(..12).unwrap_or(fingerprint),
        ),
    );
    Ok(())
}

/// `#steerinterruptexit`: every settled steering item a committed cycle left
/// unanswered, each with its typed `dispatch`, for the `session-check`
/// `steering pending` status.
///
/// Stateless on purpose: it reads no consumer watermark and writes none, so a
/// repeated `session-check` keeps reporting the same pending steering until a
/// cycle answers it, and the hook / boundary report still deliver each item
/// exactly once through their own watermarks. The base is the committed turn
/// baseline (HEAD when there is no snapshot), observed in boundary mode with no
/// debounce, excluding the closed cycle's own queue bookkeeping. Items the
/// typing gate still holds are omitted; the status line's marker carries them
/// verbatim regardless.
pub fn pending_steering_items(file: &Path, current: &str) -> Vec<SteeringItem> {
    let baseline = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten()
        .or_else(|| agent_doc_git_io::revision::show_head(file).ok().flatten());
    let Some(baseline) = baseline else {
        return Vec::new();
    };
    let base = SteeringWatermark::seed(
        &format!("committed:{}", core::content_hash(&baseline)),
        &baseline,
        None,
        Vec::new(),
    );
    let cycle = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .ok()
        .flatten();
    let owned = cycle.as_ref().map(binary_owned_ids).unwrap_or_default();
    let deterministic = agent_doc_debounce::edit_settle::DeterministicOnly;
    let ctx = ObserveContext {
        now_ms: now_ms(),
        document_changed_ms: None,
        debounce_ms: 0,
        binary_owned_queue_ids: &owned,
        explicit_send: false,
        max_hold_ms: max_hold_ms_for(file),
        classifier: &deterministic,
        median_pause_ms: None,
    };
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(current, file)
            .unwrap_or_else(|_| {
                agent_doc_frontmatter::frontmatter::parse(current)
                    .map(|(fm, _)| fm.prompt_presets)
                    .unwrap_or_default()
            });
    let mut ready = core::observe_with_mode_and_presets(
        &base,
        current,
        &ctx,
        core::ObserveMode::Boundary,
        &prompt_presets,
    )
    .ready;
    if let Some(cycle) = cycle
        && let Err(err) =
            crate::absorbed_steering::retain_operator_lineage(file, &cycle.cycle_id, &mut ready)
    {
        // This compatibility wrapper cannot return a diagnostic result. Keep
        // the contract fail-closed: surface nothing and leave a durable reason
        // for the next repair/session-check path. Stateless callers do not
        // advance a watermark, so later observations can retry.
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "pending_steering_lineage_ambiguous file={} error={err:#} (#assistantsteeringlineage)",
                file.display(),
            ),
        );
        ready.clear();
    }
    if let Err(err) = route_claimed_items(file, current, &base, &mut ready) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "pending_steering_claim_cancellation_ambiguous file={} error={err:#} (#queue-removal-cancels)",
                file.display(),
            ),
        );
        ready.clear();
    }
    crate::absorbed_steering::drop_consumed(file, &mut ready);
    ready
}

/// Whether `item` came from the exchange (a prompt only an agent cycle can
/// answer), as opposed to queue work the queue drain owns.
pub fn is_exchange_item(item: &SteeringItem) -> bool {
    item.source == core::SteeringSource::Exchange
}

/// Render [`pending_steering_items`] for the `steering pending` status: one
/// block per item, its dispatch and source first, then the operator's text
/// verbatim. `None` when there is nothing settled to list.
pub fn render_pending_steering_items(items: &[SteeringItem]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut out = String::new();
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(&format!(
            "[steering {}/{}] dispatch={} source={} change={}{}{}",
            idx + 1,
            items.len(),
            item.dispatch.as_str(),
            match item.source {
                core::SteeringSource::Exchange => "exchange",
                core::SteeringSource::Queue => "queue",
            },
            match item.change {
                core::SteeringChange::Added => "added",
                core::SteeringChange::Edited => "edited",
                core::SteeringChange::Deleted => "deleted",
            },
            if item.possibly_partial {
                " possibly_partial=true"
            } else {
                ""
            },
            item.owner
                .as_deref()
                .map(|owner| format!(" owner={owner}"))
                .unwrap_or_default(),
        ));
        if let Some(previous) = &item.previous {
            out.push_str(&format!("\nprevious: {previous}"));
        }
        let label = if item.change == core::SteeringChange::Deleted {
            "removed"
        } else {
            "verbatim"
        };
        out.push_str(&format!("\n{label}: {}", item.verbatim));
    }
    Some(out)
}

/// Render a report for the turn-boundary surface.
pub fn render_closeout(report: &SteeringReport) -> Option<String> {
    core::render_closeout_steering_context(&report.document, &report.items, report.pending)
}

/// Emit unsurfaced steering into the terminal report of `respond` /
/// `write --commit` / post-commit `session-check` (`#closeout-steering`).
///
/// The watermark advances only after `writer` accepted the rendered report.
/// Failures are reported (stderr + ops.log) and never fail the already-
/// committed closeout.
pub fn emit_closeout_steering(file: &Path, writer: &mut impl std::io::Write) {
    let outcome = (|| -> Result<bool> {
        let Some(prepared) = prepare_closeout(file)? else {
            return Ok(false);
        };
        let rendered = prepared.report.as_ref().and_then(render_closeout);
        if let Some(text) = &rendered {
            writeln!(writer, "{text}").context("write closeout steering report")?;
            writer.flush().context("flush closeout steering report")?;
        }
        prepared.acknowledge(file)?;
        Ok(rendered.is_some())
    })();
    if let Err(err) = outcome {
        eprintln!(
            "[agent-doc] WARNING: closeout steering report failed for {}; run `agent-doc steering {}` to read operator edits made during the turn: {err:#}",
            file.display(),
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "midturn_steering_closeout_error file={} error={err:#}",
                file.display()
            ),
        );
    }
}

/// Harness `PostToolUse` payload fields this hook reads. Claude Code and Codex
/// both send `session_id` and `cwd`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PostToolUseInput {
    pub session_id: String,
    pub cwd: String,
    /// Claude Code sets `agent_id` (and `agent_type`) only when the tool call
    /// ran inside a subagent; the parent's calls carry neither. Subagents
    /// share the parent's `session_id`, so without this the coordinator's
    /// steering leaked into (and was consumed by) its subagents' tool calls.
    #[serde(default)]
    pub agent_id: Option<String>,
}

impl PostToolUseInput {
    /// The tool call ran inside a subagent, not the turn that owns the
    /// document (`#steerowneronly`).
    pub fn is_subagent(&self) -> bool {
        self.agent_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
    }
}

/// Resolve the document this harness session is driving, from the binding
/// the `UserPromptSubmit` hook recorded.
pub fn session_document(input: &PostToolUseInput) -> Result<Option<PathBuf>> {
    let roots = agent_doc_codex_hook_io::project_roots_for(Path::new(&input.cwd));
    if roots.is_empty() {
        return Ok(None);
    }
    if !roots
        .iter()
        .any(|root| agent_doc_sqlite::state_store::state_db_path(root).exists())
    {
        return Ok(None);
    }
    let Some((_, state)) = agent_doc_codex_hook_io::load_state_any(&roots, &input.session_id)?
    else {
        return Ok(None);
    };
    if state.preflight_admitted == Some(false) {
        return Ok(None);
    }
    let doc = PathBuf::from(&state.doc_path);
    Ok(doc.is_file().then_some(doc))
}

/// The hook envelope Claude Code and Codex inject into the running turn.
pub fn post_tool_use_output(context: &str) -> serde_json::Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": context,
        }
    })
}

fn render_status_sync_checkpoint(
    file: &Path,
    content: &str,
    directive: &agent_doc_document::status_projection::StatusSyncDirective,
) -> String {
    let mode = match directive.mode {
        agent_doc_document::status_projection::StatusSyncMode::Additive => "additive",
        agent_doc_document::status_projection::StatusSyncMode::Replace => "replace",
    };
    format!(
        "{STATUS_SYNC_CHECKPOINT_MARKER}\n\
         Re-evaluate the persistent `agent:status` instruction now: {}\n\
         Mode: `{mode}`. If its answer changed, run `agent-doc status set '{}' --text \
         \"<current result>\"` during this turn, passing only the result: agent-doc owns \
         the `<!-- status:sync -->` markers. That focused command does not capture, \
         seal, or commit the active response cycle. If the answer did not change, \
         continue without writing. This reminder is additive to any steering context \
         above.{}",
        directive.instruction,
        file.display(),
        status_sync_evidence_suffix(file, content),
    )
}

/// `#statuspatchdrop` (c): the current managed payload, so the checkpoint is
/// answered by comparing against the document, never from memory.
///
/// `#statussyncrevert`: when the block no longer holds the latest synced value
/// (an editor undo or hand edit), the suffix also demands the re-sync.
fn status_sync_evidence_suffix(file: &Path, content: &str) -> String {
    let synced = agent_doc_snapshot_io::load_status_sync_synced_payload(file)
        .ok()
        .flatten();
    if let Some(revert) =
        agent_doc_document::status_projection::status_sync_revert(content, synced.as_deref())
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "status_sync_operator_revert_detected file={} source=midturn_checkpoint document_payload_hash={} synced_payload_hash={} action=demand_resync (#statussyncrevert)",
                file.display(),
                revert.document_payload_hash,
                revert.synced_payload_hash,
            ),
        );
    }
    agent_doc_document::status_projection::render_status_sync_checkpoint_evidence_with_synced(
        content,
        synced.as_deref(),
    )
    .map(|evidence| format!("\n{evidence}"))
    .unwrap_or_default()
}

/// Return the periodic status-sync reminder for an open owner turn.
///
/// The hook is intentionally an observation/checkpoint surface, never a second
/// document writer: resolving an arbitrary synchronization instruction requires
/// the active agent. It publishes the completed tool boundary into the live
/// controller's Lazily graph; the controller-owned Computed derives and consumes
/// the due edge. The one-shot hook neither polls nor arbitrates through SQLite.
fn status_sync_checkpoint_with(
    file: &Path,
    observed_at_ms: u64,
    observe_boundary: impl FnOnce(
        &Path,
        &Path,
        &agent_doc_controller_io::project_controller::StatusSyncCheckpointObservation,
    ) -> Result<bool>,
) -> Result<Option<String>> {
    let Some(cycle) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(None);
    };
    if !cycle_accepts_steering(&cycle, &cycle.cycle_id) {
        return Ok(None);
    }
    let content = crate::resolve_current_document_content(file, "status_sync_checkpoint")?;
    let directive = agent_doc_document::status_projection::status_sync_directive(&content)?;
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    let cycle_started_at_ms = cycle
        .cycle_id
        .strip_prefix("cycle-")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| cycle.started_at.saturating_mul(1_000));
    let interval_ms = directive
        .as_ref()
        .and_then(|directive| directive.interval_ms);
    let due = observe_boundary(
        &root,
        file,
        &agent_doc_controller_io::project_controller::StatusSyncCheckpointObservation {
            cycle_id: cycle.cycle_id.clone(),
            cycle_started_at_ms,
            interval_ms,
            observed_at_ms,
        },
    )?;
    if !due {
        return Ok(None);
    }
    let directive =
        directive.context("controller returned a due edge for a disarmed status sync")?;
    let interval_ms = directive
        .interval_ms
        .context("controller returned a due edge without a status-sync interval")?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "status_sync_checkpoint_surfaced file={} cycle={} observed_at_ms={} interval_ms={}",
            file.display(),
            cycle.cycle_id,
            observed_at_ms,
            interval_ms,
        ),
    );
    Ok(Some(render_status_sync_checkpoint(
        file, &content, &directive,
    )))
}

fn status_sync_checkpoint(file: &Path, observed_at_ms: u64) -> Result<Option<String>> {
    status_sync_checkpoint_with(
        file,
        observed_at_ms,
        agent_doc_controller_io::project_controller::observe_status_sync_checkpoint,
    )
}

fn post_tool_use_response_at_with_checkpoint(
    payload: &str,
    observed_at_ms: u64,
    checkpoint: impl FnOnce(&Path, u64) -> Result<Option<String>>,
) -> Result<Option<serde_json::Value>> {
    let input: PostToolUseInput =
        serde_json::from_str(payload).context("parse PostToolUse payload")?;
    // `#steerowneronly`: steering and persistent status checkpoints belong to
    // the turn that owns the document. A subagent's tool call advances neither.
    if input.is_subagent() {
        return Ok(None);
    }
    let Some(file) = session_document(&input)? else {
        return Ok(None);
    };
    let report = match observe(&file, CONSUMER_HOOK, true) {
        Ok(report) => report,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "midturn_steering_hook_error file={} error={err:#}",
                    file.display()
                ),
            );
            return Err(err);
        }
    };
    let mut contexts = Vec::new();
    if let Some(context) = report.and_then(|report| report.render()) {
        contexts.push(context);
    }
    match checkpoint(&file, observed_at_ms) {
        Ok(Some(context)) => contexts.push(context),
        Ok(None) => {}
        Err(err) => {
            // A checkpoint reminder must never steal already-derived steering
            // or fail the parent tool call. Leave a durable diagnostic and keep
            // the existing PostToolUse output intact.
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "status_sync_checkpoint_error file={} error={err:#}",
                    file.display()
                ),
            );
            eprintln!(
                "[agent-doc] status-sync checkpoint skipped for {}: {err:#}",
                file.display()
            );
        }
    }
    Ok((!contexts.is_empty()).then(|| post_tool_use_output(&contexts.join("\n\n"))))
}

fn post_tool_use_response_at(
    payload: &str,
    observed_at_ms: u64,
) -> Result<Option<serde_json::Value>> {
    post_tool_use_response_at_with_checkpoint(payload, observed_at_ms, status_sync_checkpoint)
}

/// Pure hook decision: the JSON to print, or `None` for silence.
pub fn post_tool_use_response(payload: &str) -> Result<Option<serde_json::Value>> {
    post_tool_use_response_at(payload, now_ms())
}

/// `agent-doc hook steering-post-tool-use` entry point. Never fails the tool
/// call: every error is reported on stderr (and ops.log when the document is
/// known) and the hook prints nothing.
pub fn handle_post_tool_use() -> Result<()> {
    use std::io::Read;
    let mut payload = String::new();
    if let Err(err) = std::io::stdin().read_to_string(&mut payload) {
        eprintln!("[agent-doc] steering hook payload read failed: {err}");
        return Ok(());
    }
    match post_tool_use_response(&payload) {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(err) => eprintln!("[agent-doc] steering hook skipped: {err:#}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LiveEditorFixture {
        file: PathBuf,
        identity: String,
        replica: agent_doc_merge::crdt_sync::ReplicaState,
    }

    impl LiveEditorFixture {
        fn new(file: &Path, editor_id: &str, content: &str) -> Self {
            let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
            agent_doc_test_support::seed_reliable_sync_editor_registration(
                &file,
                editor_id,
                &["operator_text_authority_v1", "lazily_transport_receipts_v1"],
            );
            let identity = format!("{editor_id}:{}", file.display());
            let (client_id, bootstrap) =
                agent_doc_crdt_relay_io::register_replica_for_file(&file, &identity)
                    .expect("test editor should register through the CRDT relay")
                    .expect("test editor should remain attached");
            let replica =
                agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap)
                    .expect("test editor should decode the relay bootstrap");
            let fixture = Self {
                file,
                identity,
                replica,
            };
            fixture.publish(content);
            fixture
        }

        fn publish(&self, content: &str) {
            let current = self.replica.text();
            self.replica
                .apply_local_edit(0, current.len() as u32, content);
            agent_doc_crdt_relay_io::relay_replica_update_for_file(
                &self.file,
                &self.identity,
                &self.replica.encode_state(),
            )
            .expect("test editor should publish through the CRDT relay")
            .expect("test editor update should be accepted");
        }
    }

    impl Drop for LiveEditorFixture {
        fn drop(&mut self) {
            agent_doc_crdt_relay_io::deregister_editor_replica_for_file(
                &self.file,
                &self.identity,
                std::process::id(),
            )
            .expect("test editor should deregister from the CRDT relay");
        }
    }

    #[test]
    fn hook_envelope_matches_the_harness_contract() {
        let value = post_tool_use_output("steer");
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "steer");
    }

    #[test]
    fn debounce_precedence_is_frontmatter_then_project_then_default() {
        assert_eq!(resolve_debounce_ms(Some(10), Some(20)), 10);
        assert_eq!(resolve_debounce_ms(None, Some(20)), 20);
        assert_eq!(
            resolve_debounce_ms(None, None),
            DEFAULT_STEERING_DEBOUNCE_MS
        );
    }

    #[test]
    fn hook_is_silent_outside_an_agent_doc_project() {
        let dir = tempfile::tempdir().unwrap();
        let payload = serde_json::json!({
            "session_id": "s1",
            "cwd": dir.path(),
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
        })
        .to_string();
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);
    }

    #[test]
    fn seeded_watermark_without_an_open_cycle_reports_once_per_consumer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();

        // Unchanged document: silent.
        assert_eq!(observe(&file, CONSUMER_CLI, true).unwrap(), None);

        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        // `#steeringafterclose`: with no open cycle the hook still reports the
        // unsurfaced addition, in boundary terms, exactly once.
        let hook = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("hook report");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        assert!(hook.after_close);
        assert!(
            observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .is_none_or(|report| report.items.is_empty())
        );
        // A poll reports the unsurfaced addition after close, once.
        let report = observe(&file, CONSUMER_CLI, true).unwrap().expect("report");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert!(report.after_close);
        assert_eq!(report.items[0].verbatim, "new work");
        let rerun = observe(&file, CONSUMER_CLI, true).unwrap();
        assert!(
            rerun.as_ref().is_none_or(|report| report.items.is_empty()),
            "{rerun:?}"
        );
    }

    /// `#routesyncsteer`: Run Agent Doc on a running turn sends the diff since
    /// the last send, then the next send contains only what changed after that.
    #[test]
    fn route_sync_steering_is_the_diff_since_the_last_send() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();
        record_explicit_send(&file).unwrap();

        let edited = baseline.replace("- current task\n", "- current task\n- ship the diff\n");
        std::fs::write(&file, &edited).unwrap();
        let first = prepare_route_sync_steering(&file)
            .unwrap()
            .expect("first send has a diff");
        assert!(
            first.instructions.contains("+ship the diff")
                || first.instructions.contains("ship the diff"),
            "{}",
            first.instructions
        );
        assert!(
            first.instructions.contains("diff since the last send")
                || first.instructions.contains(core::STEERING_MARKER),
            "{}",
            first.instructions
        );
        assert!(
            !first.instructions.contains("agent-doc plan.md\n"),
            "{first:?}"
        );
        record_route_sync_steering_delivered(&file, &first.cycle_id, &first.content).unwrap();
        assert!(
            prepare_route_sync_steering(&file).unwrap().is_none(),
            "the same document is not sent twice"
        );

        let later = edited.replace(
            "- ship the diff\n",
            "- ship the diff\n- and the follow-up\n",
        );
        std::fs::write(&file, &later).unwrap();
        record_explicit_send(&file).unwrap();
        let second = prepare_route_sync_steering(&file)
            .unwrap()
            .expect("second send is only the newer diff");
        assert!(
            second.instructions.contains("and the follow-up"),
            "{}",
            second.instructions
        );
        assert!(
            !second
                .instructions
                .lines()
                .any(|line| line.starts_with('+') && line.contains("ship the diff")),
            "the previous send is the baseline, not the cycle admission text: {}",
            second.instructions
        );
    }

    /// GH #173: preflight seeded the watermark from the live editor, but the
    /// PostToolUse hook compared it with stale disk. That fabricated a removal;
    /// after native save caught up, the same queue head appeared newly added.
    /// The live CRDT revision must also bypass the disk-stat gate so later
    /// unsaved edits remain observable while the file metadata is unchanged.
    #[test]
    fn live_editor_text_drives_steering_while_disk_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let stale_disk = "---\nagent_doc_steering_debounce_ms: 2500\n---\n# S\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        let live = stale_disk.replace(
            "<!-- agent:queue -->\n",
            "<!-- agent:queue priority go -->\n- 🚧 #actionable-review\n",
        );
        std::fs::write(&file, stale_disk).unwrap();
        let editor = LiveEditorFixture::new(&file, "jetbrains-gh173", &live);
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&live), Some(&live)).unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &live,
            Some("#actionable-review"),
            Vec::new(),
        )
        .unwrap();

        let first = observe(&file, CONSUMER_HOOK, true).unwrap();
        assert!(
            first.as_ref().is_none_or(|report| report.items.is_empty()),
            "the stale disk copy must not fabricate removal of the live head: {first:?}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), stale_disk);

        let live_with_addition = live.replace(
            "- 🚧 #actionable-review\n",
            "- 🚧 #actionable-review\n- follow-up task\n",
        );
        editor.publish(&live_with_addition);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            stale_disk,
            "the regression requires an unchanged disk stat and stale disk text"
        );

        let held = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("the unsaved live-editor addition should be observed");
        assert!(
            held.items.is_empty(),
            "a fresh live edit must debounce: {held:?}"
        );
        assert_eq!(held.pending, 1, "{held:?}");
        assert!(held.recheck_after_ms.is_some_and(|ms| ms <= 2500));

        record_explicit_send(&file).unwrap();
        let report = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("explicit send should flush the same authoritative bytes");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert_eq!(report.items[0].verbatim, "follow-up task");
        assert!(report.items[0].explicit, "{report:?}");

        let after_send = live_with_addition.replace(
            "- follow-up task\n",
            "- follow-up task\n- later unfinished\n",
        );
        editor.publish(&after_send);
        let passive = observe(&file, CONSUMER_HOOK, false)
            .unwrap()
            .expect("the post-send live edit should be observed");
        assert!(
            passive.items.is_empty(),
            "an older explicit send must not cover later unsaved bytes: {passive:?}"
        );
        assert_eq!(passive.pending, 1, "{passive:?}");
    }

    fn close_cycle(file: &Path, content: &str) {
        agent_doc_cycle_state_io::mark_committed(file, "test_commit", Some(content), Some(content))
            .unwrap();
    }

    fn capture_response_body(file: &Path, response_body: &str, baseline: &str) {
        let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)
            .unwrap()
            .expect("fixture should have a live cycle");
        let response_sha256 = agent_doc_hash::content_hash(response_body);
        let capture_id = state
            .capture_id
            .as_deref()
            .unwrap_or("assistant-lineage-test-capture");
        agent_doc_cycle_state_io::append_response_captured_body(
            file,
            agent_doc_cycle_state_io::CapturedResponseFactInput {
                cycle_id: &state.cycle_id,
                capture_id,
                response_sha256: &response_sha256,
                response_body,
                intent_body: None,
                mutation_plan_json: None,
                file_hash: None,
                snapshot_hash: None,
                baseline_content: Some(baseline),
            },
        )
        .unwrap();
    }

    fn assistant_lineage_fixture() -> (&'static str, &'static str, String) {
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:exchange -->\nOperator request.\n<!-- /agent:exchange -->\n";
        let response = concat!(
            "<!-- patch:exchange mode=\"append\" -->\n",
            "Implemented, dogfooded, and released.\n\n",
            "The resolved policy reports enabled=true.\n\n",
            "> **Queue prompt:** release + publish\n\n",
            "Released and publicly verified.\n",
            "<!-- /patch:exchange -->\n",
        );
        let applied = baseline.replace(
            "<!-- /agent:exchange -->",
            concat!(
                "\nImplemented, dogfooded, and released.\n\n",
                "The resolved policy reports enabled=true.\n\n",
                "> **Queue prompt:** release + publish\n\n",
                "Released and publicly verified.\n",
                "<!-- /agent:exchange -->",
            ),
        );
        (baseline, response, applied)
    }

    /// Live v0.35.518 dogfood failure: finalize committed the response, then
    /// the boundary diff classified each response paragraph as a fresh exchange
    /// addition and persisted it for the next admission. Exact capture lineage
    /// must remove every assistant-authored item before report/ledger effects.
    #[test]
    fn committed_captured_response_is_not_replayed_as_operator_steering() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let (baseline, response, applied) = assistant_lineage_fixture();
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(&file, &cycle.cycle_id, baseline, None, Vec::new()).unwrap();
        std::fs::write(&file, &applied).unwrap();
        capture_response_body(&file, response, baseline);
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &applied,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        close_cycle(&file, &applied);

        let mut out = Vec::new();
        emit_closeout_steering(&file, &mut out);
        assert!(out.is_empty(), "{}", String::from_utf8_lossy(&out));
        assert!(
            crate::absorbed_steering::unanswered_prompts(&file)
                .unwrap()
                .is_empty(),
            "assistant response must not enter the absorbed-steering ledger"
        );
        assert!(pending_steering_items(&file, &applied).is_empty());

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("midturn_steering_binary_response_suppressed"),
            "{ops}"
        );
    }

    /// v0.35.527 dogfood failure: a closeout's committed response opened with
    /// `> **Queue prompt:** <line>` strike echoes above its `### Re:` heading.
    /// A second `respond` then auto-reopened a fresh out-of-band cycle from
    /// HEAD, which never re-seeds the steering watermark, and committed a
    /// second response. The hook's watermark still described the first cycle,
    /// but the lineage filter subtracted only the LATEST cycle's captured
    /// response, so every first-response echo surfaced as a new operator
    /// exchange prompt. Every response the binary captured since the
    /// watermark's cycle is agent-authored and must be subtracted.
    #[test]
    fn earlier_cycle_response_echoes_are_not_operator_steering_after_auto_reopen() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:exchange -->\nOperator request.\n<!-- /agent:exchange -->\n";
        let first_body = concat!(
            "> **Chat prompt (#chatprompt):** check the open issues\n\n",
            "> **Queue prompt:** fix https://github.com/example/repo/issues/317\n\n",
            "> **Queue prompt:** fix https://github.com/example/repo/issues/318\n\n",
            "> **Queue prompt:** release + publish\n\n",
            "### Re: closeout\n\n",
            "Fixed and pushed.\n",
        );
        let first_response = format!(
            "<!-- patch:exchange mode=\"append\" -->\n{first_body}<!-- /patch:exchange -->\n"
        );
        let first_applied = baseline.replace(
            "<!-- /agent:exchange -->",
            &format!("\n{first_body}<!-- /agent:exchange -->"),
        );
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(&file, &cycle.cycle_id, baseline, None, Vec::new()).unwrap();
        std::fs::write(&file, &first_applied).unwrap();
        capture_response_body(&file, &first_response, baseline);
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &first_applied,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        close_cycle(&file, &first_applied);

        // The second `respond` auto-reopens from HEAD without a preflight seed.
        let reopened = agent_doc_cycle_state_io::start_out_of_band_closeout_preflight(
            &file,
            Some(&first_applied),
            Some(&first_applied),
        )
        .unwrap();
        assert_ne!(reopened.cycle_id, cycle.cycle_id);
        let second_body = "### Re: follow-up closeout\n\nSecond closeout recorded.\n";
        let second_response = format!(
            "<!-- patch:exchange mode=\"append\" -->\n{second_body}<!-- /patch:exchange -->\n"
        );
        let second_applied = first_applied.replace(
            "<!-- /agent:exchange -->",
            &format!("\n{second_body}<!-- /agent:exchange -->"),
        );
        std::fs::write(&file, &second_applied).unwrap();
        capture_response_body(&file, &second_response, &first_applied);
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &second_applied,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        close_cycle(&file, &second_applied);

        let hook = observe(&file, CONSUMER_HOOK, true).unwrap();
        assert!(
            hook.as_ref().is_none_or(|report| report.items.is_empty()),
            "agent-authored echoes surfaced as operator steering: {hook:?}"
        );
        assert!(
            crate::absorbed_steering::unanswered_prompts(&file)
                .unwrap()
                .is_empty(),
            "agent-authored echoes must not enter the absorbed-steering ledger"
        );

        // An operator prompt typed below the re-anchored boundary after both
        // closeouts still surfaces.
        let operator_prompt = "Also check the release notes.";
        std::fs::write(
            &file,
            second_applied.replace(
                "<!-- /agent:exchange -->",
                &format!(
                    "<!-- agent:boundary:echosteer -->\n{operator_prompt}\n<!-- /agent:exchange -->"
                ),
            ),
        )
        .unwrap();
        let hook = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("operator prompt should produce a report");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        assert_eq!(hook.items[0].verbatim, operator_prompt);
    }

    #[test]
    fn captured_response_filter_preserves_adjacent_operator_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let (baseline, response, applied) = assistant_lineage_fixture();
        let operator_prompt = "Also verify that the queue stayed synchronized.";
        let applied = applied.replace(
            "<!-- /agent:exchange -->",
            &format!("\n{operator_prompt}\n<!-- /agent:exchange -->"),
        );
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(&file, &cycle.cycle_id, baseline, None, Vec::new()).unwrap();
        std::fs::write(&file, &applied).unwrap();
        capture_response_body(&file, response, baseline);
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &applied,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        close_cycle(&file, &applied);

        let prepared = prepare_closeout(&file)
            .unwrap()
            .expect("operator edit should produce a boundary observation");
        let report = prepared.report.as_ref().expect("operator report");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert_eq!(report.items[0].verbatim, operator_prompt);
        prepared.acknowledge(&file).unwrap();
        assert_eq!(
            crate::absorbed_steering::unanswered_prompts(&file).unwrap(),
            vec![operator_prompt.to_string()]
        );
    }

    #[test]
    fn missing_captured_response_body_fails_closed_without_advancing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let (baseline, response, applied) = assistant_lineage_fixture();
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(&file, &cycle.cycle_id, baseline, None, Vec::new()).unwrap();
        std::fs::write(&file, &applied).unwrap();
        let response_sha256 = agent_doc_hash::content_hash(response);
        agent_doc_cycle_state_io::mark_response_captured(
            &file,
            "assistant-lineage-test-capture",
            Some(&applied),
            Some(&applied),
            &response_sha256,
            Some(&cycle.cycle_id),
        )
        .unwrap();
        close_cycle(&file, &applied);

        let err = prepare_closeout(&file).unwrap_err();
        assert!(
            format!("{err:#}").contains("missing captured-response body"),
            "{err:#}"
        );

        // Repairing the exact lineage lets the same unadvanced observation be
        // retried and then suppressed normally.
        capture_response_body(&file, response, baseline);
        let prepared = prepare_closeout(&file)
            .unwrap()
            .expect("watermark must remain retryable");
        assert!(
            prepared
                .report
                .as_ref()
                .is_none_or(|report| report.items.is_empty()),
            "{:?}",
            prepared.report
        );
    }

    /// The operator report: a `#subagents` queue addition made after the seed,
    /// with no PostToolUse hook ever running, must reach the agent in the
    /// `respond` terminal report exactly once, as a dispatch directive; a
    /// `steering` poll after close must still show it.
    #[test]
    fn closeout_report_carries_a_hookless_subagent_addition_exactly_once() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 600000\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        // The closeout consumed the current head; the operator's line stays.
        let closed = baseline.replace("- current task\n", "- #subagents do [#preflightdeadline]\n");
        std::fs::write(&file, &closed).unwrap();
        // A closeout that deferred (cycle still open) does not report yet and
        // does not advance the watermark.
        let mut deferred = Vec::new();
        emit_closeout_steering(&file, &mut deferred);
        assert!(
            deferred.is_empty(),
            "{}",
            String::from_utf8_lossy(&deferred)
        );
        close_cycle(&file, &closed);

        let mut out = Vec::new();
        emit_closeout_steering(&file, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with(core::CLOSEOUT_STEERING_MARKER), "{text}");
        assert!(text.contains("dispatch=subagent"), "{text}");
        assert!(
            text.contains("verbatim: #subagents do [#preflightdeadline]"),
            "{text}"
        );
        assert!(text.contains("DISPATCH NOW"), "{text}");
        assert!(
            text.contains("agent-doc queue claim") && text.contains("--item '#preflightdeadline'"),
            "{text}"
        );
        assert!(
            !text.contains("REMOVED the queue item"),
            "consuming the finished head is not steering: {text}"
        );
        assert_eq!(text.matches("[steering ").count(), 1, "{text}");

        // Exactly once on the agent channel: a second closeout (e.g. the
        // post-commit session-check) is silent.
        let mut again = Vec::new();
        emit_closeout_steering(&file, &mut again);
        assert!(again.is_empty(), "{}", String::from_utf8_lossy(&again));

        // A steering poll after close still shows it (its own consumer) once
        // the operator's edit is quiet for the debounce window.
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
        let peek = observe(&file, CONSUMER_CLI, false).unwrap().expect("peek");
        assert_eq!(peek.items.len(), 1, "{peek:?}");
        assert!(peek.after_close);
        assert_eq!(peek.items[0].dispatch, core::SteeringDispatch::Subagent);
        assert!(peek.render().unwrap().contains("DISPATCH NOW"));
        // Peek never advances.
        assert_eq!(
            observe(&file, CONSUMER_CLI, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
    }

    /// GH #162: stale-lock repair marked the cycle committed while the selected
    /// head and owning harness turn were still open. Explicit steering must use
    /// in-turn wording, and the closeout renderer must remain silent.
    #[test]
    fn explicit_send_to_terminalized_cycle_with_live_head_addresses_now() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 600000\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let sent = baseline.replace(
            "- current task\n",
            "- 🚧 current task\n- publish the release\n",
        );
        std::fs::write(&file, &sent).unwrap();
        record_explicit_send(&file).unwrap();
        // Models repair_preflight_stale_lock committing marker bookkeeping
        // without consuming/responding to the selected head.
        close_cycle(&file, &sent);

        let mut closeout = Vec::new();
        emit_closeout_steering(&file, &mut closeout);
        assert!(
            closeout.is_empty(),
            "{}",
            String::from_utf8_lossy(&closeout)
        );

        let report = observe(&file, CONSUMER_HOOK, false)
            .unwrap()
            .expect("in-turn steering report");
        assert!(!report.after_close, "{report:?}");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert!(report.items[0].explicit);
        assert_eq!(report.items[0].dispatch, core::SteeringDispatch::AddressNow);
        let rendered = report.render().unwrap();
        assert!(rendered.starts_with(core::STEERING_MARKER), "{rendered}");
        assert!(
            !rendered.contains("Your response is already committed"),
            "{rendered}"
        );
        assert!(rendered.contains("Address it in THIS turn"), "{rendered}");
    }

    #[test]
    fn closeout_report_skips_what_the_hook_already_surfaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let mid = baseline.replace("- current task\n", "- current task\n- first addition\n");
        std::fs::write(&file, &mid).unwrap();
        assert_eq!(
            observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
        let late = mid
            .replace("- current task\n", "")
            .replace("- first addition\n", "- first addition\n- late addition\n");
        std::fs::write(&file, &late).unwrap();
        close_cycle(&file, &late);
        let mut out = Vec::new();
        emit_closeout_steering(&file, &mut out);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("verbatim: late addition"), "{text}");
        assert!(!text.contains("first addition"), "{text}");
        assert!(text.contains("dispatch=drain_after_current"), "{text}");
    }

    #[test]
    fn open_cycle_surfaces_steering_once_through_the_hook_consumer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        let report = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("report");
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].verbatim, "new work");
        let context = report.render().unwrap();
        assert!(
            context.contains("dispatch=drain_after_current"),
            "{context}"
        );
        // Exactly once: the same document never re-injects.
        assert_eq!(observe(&file, CONSUMER_HOOK, true).unwrap(), None);
        // Consumers are independent: the CLI still sees it once.
        assert_eq!(
            observe(&file, CONSUMER_CLI, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn post_tool_use_hook_emits_the_harness_envelope_then_stays_silent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        agent_doc_codex_hook_io::save_state(
            &root,
            &agent_doc_codex_hook_io::SessionState {
                session_id: "sess-1".into(),
                identity_origin: agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                harness: None,
                doc_path: file.display().to_string(),
                last_turn_id: String::new(),
                last_prompt: "agent-doc plan.md".into(),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: None,
                updated_at: 1,
            },
        )
        .unwrap();
        let payload = serde_json::json!({
            "session_id": "sess-1",
            "cwd": root,
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo build"},
            "tool_response": {"stdout": "ok"},
        })
        .to_string();
        // No seeded cycle yet: silent.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);

        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        // Seeded, nothing new: silent.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);

        std::fs::write(
            &file,
            baseline.replace(
                "- current task\n",
                "- current task\n- #subagents fix issue 111\n",
            ),
        )
        .unwrap();
        let output = post_tool_use_response(&payload)
            .unwrap()
            .expect("steering output");
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        let context = output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.starts_with(core::STEERING_MARKER), "{context}");
        assert!(context.contains("dispatch=subagent"), "{context}");
        assert!(context.contains("#subagents fix issue 111"), "{context}");
        // Exactly once.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);
    }

    fn backdate(file: &Path) {
        std::fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
            .unwrap();
    }

    /// The 2026-10-03 incident (`#steeringwake`): the cycle closed, the agent
    /// went idle, and the operator queued `#subagent: <issue>` items. The idle
    /// wake must see them, must not consume them (the woken turn's hook or
    /// boundary report still delivers them exactly once), and must skip items
    /// a worker already claimed.
    #[test]
    fn idle_wake_sees_post_close_additions_without_consuming_and_skips_claims() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 600000\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- release + publish\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let closed = baseline.replace("- current task\n", "");
        std::fs::write(&file, &closed).unwrap();
        close_cycle(&file, &closed);
        let mut report = Vec::new();
        emit_closeout_steering(&file, &mut report);

        // Nothing new since the boundary report: no wake.
        assert!(observe_for_wake(&file).unwrap().items.is_empty());

        let added = closed.replace(
            "- release + publish\n",
            "- #subagent: https://github.com/btakita/agent-doc/issues/116\n\
             - #subagent: https://github.com/btakita/agent-doc/issues/117\n\
             - release + publish\n",
        );
        std::fs::write(&file, &added).unwrap();
        // Still being typed (inside the debounce window): held, with exactly
        // one scheduled re-observation instead of a poll.
        let typing = observe_for_wake(&file).unwrap();
        assert!(typing.items.is_empty(), "{typing:?}");
        assert_eq!(typing.pending, 2);
        assert!(typing.recheck_after_ms.is_some_and(|ms| ms <= 600_000));

        backdate(&file);
        let wake = observe_for_wake(&file).unwrap();
        assert_eq!(wake.items.len(), 2, "{wake:?}");
        assert!(
            wake.items
                .iter()
                .all(|item| item.dispatch == core::SteeringDispatch::Subagent)
        );
        assert!(!wake.fingerprint.is_empty());
        // Non-consuming: the same set, the same fingerprint.
        assert_eq!(observe_for_wake(&file).unwrap(), wake);

        // A worker claims one: it is in flight elsewhere and never wakes.
        agent_doc_queue_io::queue_claim::claim(
            &file,
            "#subagent: https://github.com/btakita/agent-doc/issues/116",
            "subagent:gh-116",
            3600,
        )
        .unwrap();
        let after_claim = observe_for_wake(&file).unwrap();
        assert_eq!(after_claim.items.len(), 1, "{after_claim:?}");
        assert!(after_claim.items[0].verbatim.ends_with("/117"));
        assert_ne!(after_claim.fingerprint, wake.fingerprint);

        // The agent channel still delivers both, once: the wake ate nothing.
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(hook.items.len(), 2, "{hook:?}");
        assert!(hook.after_close);
        // ...and once surfaced there, nothing is left to wake for.
        assert!(observe_for_wake(&file).unwrap().items.is_empty());

        // `#claimdispatchidentity`: the operator re-tags the CLAIMED line
        // while its subagent works. The edit is the same work, still claimed,
        // so it never wakes the session as new steering.
        let retagged = added.replace(
            "- #subagent: https://github.com/btakita/agent-doc/issues/116\n",
            "- #subagents: #gh-fix https://github.com/btakita/agent-doc/issues/116\n",
        );
        assert_ne!(retagged, added);
        std::fs::write(&file, &retagged).unwrap();
        backdate(&file);
        let after_edit = observe_for_wake(&file).unwrap();
        assert!(after_edit.items.is_empty(), "{after_edit:?}");

        // Changing the claimed line's issue in place wakes the coordinator,
        // routed to the claim's owner (`#claimfollowedit`): the claim follows
        // its line, so the owner decides, and no second worker is offered.
        let retargeted = retagged.replace("issues/116", "issues/118");
        std::fs::write(&file, &retargeted).unwrap();
        backdate(&file);
        let after_retarget = observe_for_wake(&file).unwrap();
        assert_eq!(after_retarget.items.len(), 1, "{after_retarget:?}");
        assert!(after_retarget.items[0].verbatim.ends_with("/118"));
        assert_eq!(
            after_retarget.items[0].dispatch,
            core::SteeringDispatch::ForwardToOwner,
            "{after_retarget:?}"
        );
        assert_eq!(
            after_retarget.items[0].owner.as_deref(),
            Some("subagent:gh-116")
        );
    }

    /// Pin a document's mtime in the future: the settle gate then has no
    /// document-quiet evidence (quiet = 0), exactly like a live editor-attached
    /// document whose edit timestamp is unknown (`document_changed_ms = None`).
    fn no_quiet_evidence(file: &Path) {
        std::fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
            .unwrap();
    }

    /// `#fixthrashingbackground`: the idle wake must not poll a held steering
    /// candidate forever.
    ///
    /// Live 2026-10-12 (haiven-dev): with an editor attached the settle gate has
    /// no edit timestamp, and the wake never advances its watermark, so every
    /// observation saw the candidate for the first time, held it, and asked for
    /// another re-observation — a full-document controller read each time, on
    /// every idle supervisor, indefinitely. The wake's in-process carry is the
    /// settle evidence: the candidate settles after one window and the
    /// re-observation requests stop.
    #[test]
    fn idle_wake_settles_a_held_candidate_instead_of_rechecking_forever() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        // A short max hold: whichever gate answers (the learned one is on by
        // default and may call the item still-being-typed), a candidate with
        // settle evidence is delivered within it.
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_steering_max_hold_ms = 300\n",
        )
        .unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 200\n---\n# S\n\n<!-- agent:queue -->\n- current task\n- release\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let closed = baseline.replace("- current task\n", "");
        std::fs::write(&file, &closed).unwrap();
        close_cycle(&file, &closed);
        emit_closeout_steering(&file, &mut Vec::new());
        let added = closed.replace("- release\n", "- fix the flaky login test.\n- release\n");
        std::fs::write(&file, &added).unwrap();
        no_quiet_evidence(&file);
        let window = std::time::Duration::from_millis(350);

        // Without settle evidence (the pre-fix wake): held on every
        // observation, each one asking to be re-observed. This is the poll.
        for _ in 0..3 {
            let observation = observe_for_wake(&file).unwrap();
            assert!(observation.items.is_empty(), "{observation:?}");
            assert_eq!(observation.pending, 1, "{observation:?}");
            assert!(observation.recheck_after_ms.is_some(), "{observation:?}");
            std::thread::sleep(window);
        }

        // With the supervisor's carry: held once, then settled, then silent.
        let mut carry = WakeSettleCarry::default();
        let first = observe_for_wake_with(&file, &mut carry).unwrap();
        assert!(first.items.is_empty(), "{first:?}");
        assert!(first.recheck_after_ms.is_some(), "{first:?}");
        assert_eq!(carry.tracked(), 1);
        std::thread::sleep(window);
        let settled = observe_for_wake_with(&file, &mut carry).unwrap();
        assert_eq!(settled.items.len(), 1, "{settled:?}");
        assert_eq!(settled.recheck_after_ms, None, "{settled:?}");
        // Idle and unchanged: no further re-observation is ever requested, and
        // the wake still does not consume the item.
        for _ in 0..3 {
            let again = observe_for_wake_with(&file, &mut carry).unwrap();
            assert_eq!(again.recheck_after_ms, None, "{again:?}");
            assert_eq!(again.fingerprint, settled.fingerprint);
        }
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(hook.items.len() + hook.pending, 1, "{hook:?}");
    }

    /// The carry belongs to one watermark: a re-seeded cycle starts fresh.
    #[test]
    fn wake_settle_carry_never_crosses_a_reseed() {
        let mut carry = WakeSettleCarry::default();
        let mut first = SteeringWatermark::seed("cycle-1", "base", None, Vec::new());
        first.pending.insert(
            "k".to_string(),
            core::PendingObservation {
                content_hash: "h".to_string(),
                first_seen_ms: 1,
                item_started_ms: None,
                item_started_chars: None,
            },
        );
        carry.remember(Some(&first));
        let mut same = SteeringWatermark::seed("cycle-1", "base", None, Vec::new());
        carry.merge_into(&mut same);
        assert_eq!(same.pending.len(), 1);
        let mut reseeded = SteeringWatermark::seed("cycle-2", "base", None, Vec::new());
        carry.merge_into(&mut reseeded);
        assert!(reseeded.pending.is_empty());
        carry.remember(None);
        assert_eq!(carry.tracked(), 0);
    }

    /// The operator's complaint: a diagnostic `agent-doc steering` read
    /// consumed steering. A consuming poll must never suppress the idle wake.
    #[test]
    fn a_consuming_steering_poll_never_suppresses_the_idle_wake() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        backdate(&file);
        assert_eq!(observe_for_wake(&file).unwrap().items.len(), 1);
        assert_eq!(
            observe(&file, CONSUMER_CLI, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(observe_for_wake(&file).unwrap().items.len(), 1);
    }

    /// `#steergatelog`: every consuming observation records its settle
    /// decisions with features, and the outcome labels attach from what the
    /// operator does next: the held fragment was right to hold (on time), the
    /// delivered line the operator immediately re-edited was premature.
    #[test]
    fn consuming_observations_log_each_settle_decision_with_its_outcome() {
        use crate::steering_gate_log::{GateLabel, GatePhase, export_dataset};
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent: codex\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();
        let with = |line: &str| {
            baseline.replace("- current task\n", &format!("- current task\n- {line}\n"))
        };

        std::fs::write(&file, with("Should we release + publish the")).unwrap();
        let held = observe(&file, CONSUMER_CLI, true).unwrap().unwrap();
        assert!(held.items.is_empty(), "{:?}", held.items);
        std::fs::write(&file, with("Should we release + publish the C++ bindings?")).unwrap();
        let delivered = observe(&file, CONSUMER_CLI, true).unwrap().unwrap();
        assert_eq!(delivered.items.len(), 1, "{:?}", delivered.items);
        std::fs::write(
            &file,
            with("Should we release + publish the C++ bindings? Tag it too."),
        )
        .unwrap();
        observe(&file, CONSUMER_CLI, true).unwrap().unwrap();

        let rows = export_dataset(dir.path(), None, now_ms(), 45_000).unwrap();
        let fragment = rows
            .iter()
            .find(|r| r.row.phase != GatePhase::Delivered)
            .expect("the held fragment is logged");
        assert_eq!(fragment.row.document, "plan.md");
        assert_eq!(fragment.row.consumer, CONSUMER_CLI);
        assert_eq!(fragment.row.harness, "codex");
        assert_eq!(
            fragment.row.features.trailing_token,
            agent_doc_debounce::edit_settle::TrailingToken::Article
        );
        assert_eq!(
            fragment.row.tier,
            agent_doc_debounce::edit_settle::SettleTier::HeldUnfinished
        );
        assert!(fragment.row.superseded_at_ms.is_some());
        assert_eq!(fragment.row.label, Some(GateLabel::OnTime));
        let first_delivery = rows
            .iter()
            .find(|r| {
                r.row.phase == GatePhase::Delivered
                    && r.row.text_hash
                        == core::gate_text_hash("Should we release + publish the C++ bindings?")
            })
            .expect("the delivery is logged");
        assert!(first_delivery.row.re_edited_at_ms.is_some());
        assert_eq!(first_delivery.row.label, Some(GateLabel::Premature));
    }

    #[test]
    fn wake_receipt_round_trips_through_state_db() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "# S\n").unwrap();
        assert_eq!(load_wake_receipt(&file).unwrap(), None);
        record_wake_receipt(&file, "abc123", 2).unwrap();
        assert_eq!(load_wake_receipt(&file).unwrap().as_deref(), Some("abc123"));
    }

    fn bind_session(root: &Path, file: &Path, session_id: &str) {
        agent_doc_codex_hook_io::save_state(
            root,
            &agent_doc_codex_hook_io::SessionState {
                session_id: session_id.to_string(),
                identity_origin: agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                harness: None,
                doc_path: file.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("/agent-doc {}", file.display()),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: Some(true),
                updated_at: 1,
            },
        )
        .unwrap();
    }

    fn hook_payload(root: &Path, agent_id: Option<&str>) -> String {
        let mut payload = serde_json::json!({
            "session_id": "coord",
            "cwd": root,
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
        });
        if let Some(agent_id) = agent_id {
            payload["agent_id"] = serde_json::json!(agent_id);
            payload["agent_type"] = serde_json::json!("general-purpose");
        }
        payload.to_string()
    }

    #[test]
    fn status_sync_checkpoint_is_owner_only_periodic_and_additive_to_steering() {
        use std::cell::RefCell;
        use std::collections::VecDeque;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md");
        let baseline = concat!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n",
            "<!-- agent:status sync=\"Keep the published release current\" interval=5m -->\n",
            "operator note\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        bind_session(root, &file, "coord");
        let owner_payload = hook_payload(root, None);
        let started_at_ms = cycle
            .cycle_id
            .strip_prefix("cycle-")
            .unwrap()
            .parse::<u64>()
            .unwrap();
        let configured_interval_ms = 5 * 60_000;
        let first_due = started_at_ms + configured_interval_ms;
        let decisions = RefCell::new(VecDeque::from([false, true, false, true, false]));
        let observations = RefCell::new(Vec::new());
        let checkpoint = |file: &Path, observed_at_ms: u64| {
            status_sync_checkpoint_with(file, observed_at_ms, |_, _, observation| {
                observations.borrow_mut().push(observation.clone());
                Ok(decisions
                    .borrow_mut()
                    .pop_front()
                    .expect("one mocked controller decision per owner boundary"))
            })
        };

        // Preflight already supplied the contract; an immediate first tool call
        // must not duplicate it.
        assert_eq!(
            post_tool_use_response_at_with_checkpoint(&owner_payload, first_due - 1, checkpoint,)
                .unwrap(),
            None
        );

        // A subagent cannot receive or consume the coordinator's status edge.
        assert_eq!(
            post_tool_use_response_at_with_checkpoint(
                &hook_payload(root, Some("agent-a1b2")),
                first_due,
                checkpoint,
            )
            .unwrap(),
            None
        );
        // Hooks cannot interrupt a continuously running tool. Once overdue,
        // the next completed owner tool call surfaces the reminder immediately.
        let first_observed = first_due + 42_000;
        let first =
            post_tool_use_response_at_with_checkpoint(&owner_payload, first_observed, checkpoint)
                .unwrap()
                .expect("first status checkpoint");
        let first_context = first["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(first_context.contains(STATUS_SYNC_CHECKPOINT_MARKER));
        assert!(first_context.contains("Keep the published release current"));
        assert!(first_context.contains("agent-doc status set"));
        assert!(first_context.contains("does not capture, seal, or commit"));
        // `#statuspatchdrop` (c): the reminder quotes the document's current
        // status state instead of leaving the agent to answer from memory.
        assert!(
            first_context.contains("has no `status:sync` payload yet"),
            "{first_context}"
        );
        assert!(
            first_context.contains("1 line(s) of ordinary prose"),
            "{first_context}"
        );

        // Multiple tool calls within one full interval produce one reminder.
        assert_eq!(
            post_tool_use_response_at_with_checkpoint(&owner_payload, first_due + 1, checkpoint,)
                .unwrap(),
            None
        );

        // At the next checkpoint, ordinary operator steering remains present;
        // status sync is additive rather than replacing the steering context.
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- inspect issue 42\n"),
        )
        .unwrap();
        let second_due = first_observed + configured_interval_ms;
        let combined =
            post_tool_use_response_at_with_checkpoint(&owner_payload, second_due, checkpoint)
                .unwrap()
                .expect("combined steering and status checkpoint");
        let combined_context = combined["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(combined_context.contains(core::STEERING_MARKER));
        assert!(combined_context.contains("inspect issue 42"));
        assert!(combined_context.contains(STATUS_SYNC_CHECKPOINT_MARKER));

        // Removing the opt-in interval leaves sync active for the next
        // preflight but disables all periodic PostToolUse checkpoints.
        std::fs::write(&file, baseline.replace(" interval=5m", "")).unwrap();
        assert_eq!(
            checkpoint(&file, second_due + configured_interval_ms).unwrap(),
            None
        );
        assert_eq!(
            observations.borrow().last().unwrap().interval_ms,
            None,
            "default-off sync must publish an explicit disarm fact"
        );

        // Captured/closed cycles no longer accept mid-turn status reminders.
        close_cycle(&file, &std::fs::read_to_string(&file).unwrap());
        let observed_boundaries = observations.borrow().len();
        assert_eq!(
            post_tool_use_response_at_with_checkpoint(
                &owner_payload,
                second_due + configured_interval_ms,
                checkpoint,
            )
            .unwrap(),
            None
        );
        assert_eq!(observations.borrow().len(), observed_boundaries);
    }

    /// `#steerowneronly` (#steerworks): a subagent's tool calls share the
    /// coordinator's `session_id`. They must neither receive the coordinator's
    /// steering nor consume it; the coordinator's own next tool call gets it.
    #[test]
    fn subagent_tool_calls_never_receive_or_consume_coordinator_steering() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        bind_session(root, &file, "coord");
        std::fs::write(
            &file,
            baseline.replace(
                "- current task\n",
                "- current task\n- #subagents: https://github.com/btakita/agent-doc/issues/127\n",
            ),
        )
        .unwrap();
        backdate(&file);

        for _ in 0..2 {
            assert_eq!(
                post_tool_use_response(&hook_payload(root, Some("agent-a1b2"))).unwrap(),
                None,
                "a subagent tool call must stay silent"
            );
        }
        let delivered = post_tool_use_response(&hook_payload(root, None))
            .unwrap()
            .expect("the coordinator's own tool call delivers the item");
        let context = delivered["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.contains("issues/127"), "{context}");
        assert_eq!(
            post_tool_use_response(&hook_payload(root, None)).unwrap(),
            None
        );
    }

    /// `#claimedsteerwake`: `Run Agent Doc` / `agent-doc route` is an explicit
    /// send. A line the typing gate would hold (unfinished shape, edited a
    /// moment ago) is delivered at once, final, marked `sent=explicit`.
    #[test]
    fn explicit_send_flushes_pending_steering_past_the_typing_gate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 600000\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- also publish the\n"),
        )
        .unwrap();
        let held = observe(&file, CONSUMER_HOOK, false).unwrap().unwrap();
        assert!(held.items.is_empty(), "passive delivery holds it: {held:?}");
        assert_eq!(held.pending, 1);

        record_explicit_send(&file).unwrap();
        let sent = observe(&file, CONSUMER_HOOK, true).unwrap().unwrap();
        assert_eq!(sent.items.len(), 1, "{sent:?}");
        assert!(sent.items[0].explicit && !sent.items[0].possibly_partial);
        assert!(sent.render().unwrap().contains("sent=explicit"));
        assert!(
            observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .is_none_or(|report| report.items.is_empty()),
            "exactly once"
        );

        // An edit typed after the send (past the save grace) is passive again.
        let later = std::fs::read_to_string(&file).unwrap().replace(
            "- also publish the\n",
            "- also publish the\n- and then the\n",
        );
        std::fs::write(&file, later).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(
                std::time::SystemTime::now()
                    + std::time::Duration::from_millis(EXPLICIT_SEND_SAVE_GRACE_MS + 60_000),
            )
            .unwrap();
        let passive = observe(&file, CONSUMER_HOOK, false).unwrap().unwrap();
        assert!(passive.items.is_empty(), "{passive:?}");
        assert_eq!(passive.pending, 1);
    }

    /// `#claimfollowedit` (agent-doc-bugs.md, 2026-10-15): a claimed queue line
    /// whose URL the operator corrects in place stays claimed by its owner, and
    /// steering forwards the edit to that owner instead of offering a second
    /// subagent. The claimed text was on two identical lines: editing one copy
    /// forwards to the owner, deleting the other copy keeps the claim on the
    /// edited line, and only deleting the line itself releases the claim.
    #[test]
    fn in_place_edit_of_a_claimed_duplicate_line_keeps_its_claim_and_forwards_to_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let foc = "#subagents: #gh-fix https://github.com/btakita/agent-foc/issues/272";
        let fixed = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/272";
        let doc = |lines: &[&str]| {
            let queue: String = lines.iter().map(|line| format!("- {line}\n")).collect();
            format!(
                "---\nagent_doc_steering_debounce_ms: 2500\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue go -->\n{queue}<!-- /agent:queue -->\n"
            )
        };
        let baseline = doc(&["current task", foc, "release + publish", foc]);
        std::fs::write(&file, &baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&file, foc, "subagent:gh272", 3600).unwrap();
        assert_eq!(
            claim.anchor.as_ref().map(|anchor| anchor.slots.len()),
            Some(2),
            "one claim binds both identical lines"
        );
        agent_doc_queue_io::queue_claim::start_attempt(
            &file,
            foc,
            "subagent:gh272",
            "worker:gh272",
            3600,
            claim.generation,
        )
        .unwrap();

        // The operator corrects the first copy in place.
        let one_fixed = doc(&["current task", fixed, "release + publish", foc]);
        std::fs::write(&file, &one_fixed).unwrap();
        backdate(&file);
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        assert_eq!(
            hook.items[0].dispatch,
            core::SteeringDispatch::ForwardToOwner,
            "{hook:?}"
        );
        assert_eq!(hook.items[0].owner.as_deref(), Some("subagent:gh272"));
        assert_eq!(hook.items[0].previous.as_deref(), Some(foc));

        // ...and deletes the stale copy: the claim moves to the edited line.
        let cleaned = doc(&["current task", fixed, "release + publish"]);
        std::fs::write(&file, &cleaned).unwrap();
        backdate(&file);
        let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(&file, &cleaned);
        assert!(claimed.claims(fixed), "the claim followed the edited line");
        assert_eq!(
            agent_doc_queue_io::queue_claim::prune_closed_claims(&file, &cleaned).unwrap(),
            0,
            "closeout must not prune the edited line's claim"
        );
        let stored = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(stored.claims.len(), 1, "{stored:?}");
        assert_eq!(stored.claims[0].owner, "subagent:gh272");
        assert_eq!(stored.claims[0].item_text, fixed);

        // A further in-place edit still routes to the owner, with no re-claim.
        let annotated_line = format!("{fixed} (Coder env)");
        let annotated = doc(&["current task", &annotated_line, "release + publish"]);
        std::fs::write(&file, &annotated).unwrap();
        backdate(&file);
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert!(
            hook.items
                .iter()
                .all(|item| item.dispatch == core::SteeringDispatch::ForwardToOwner),
            "{hook:?}"
        );

        // The worker refreshes by the text it was handed.
        agent_doc_queue_io::queue_claim::refresh_attempt(
            &file,
            foc,
            "subagent:gh272",
            3600,
            Some(claim.generation),
        )
        .unwrap();

        // Deleting the line releases the claim.
        let deleted = doc(&["current task", "release + publish"]);
        std::fs::write(&file, &deleted).unwrap();
        assert_eq!(
            agent_doc_queue_io::queue_claim::prune_closed_claims(&file, &deleted).unwrap(),
            1
        );
        assert!(
            agent_doc_queue_io::queue_claim::load_ledger(&file)
                .unwrap()
                .claims
                .is_empty()
        );
    }

    /// `#queue-removal-cancels`: deleting a claimed non-current row is not
    /// silent queue cleanup. It identifies the exact bound worker attempt and
    /// gives the harness an authoritative, generation-fenced cancellation.
    #[test]
    fn removing_claimed_head_cancels_the_exact_worker_attempt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let head = "release + publish: include issue 348";
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:release-348", 3600)
                .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &file,
            head,
            "subagent:release-348",
            "/root/release_348",
            3600,
            claim.generation,
        )
        .unwrap();

        let deleted = baseline.replace(&format!("- {head}\n"), "");
        std::fs::write(&file, deleted).unwrap();
        backdate(&file);
        let report = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(report.items.len(), 1, "{report:?}");
        let item = &report.items[0];
        assert_eq!(item.change, core::SteeringChange::Deleted);
        assert_eq!(item.dispatch, core::SteeringDispatch::CancelClaimed);
        assert_eq!(item.owner.as_deref(), Some("subagent:release-348"));
        assert_eq!(item.worker_id.as_deref(), Some("/root/release_348"));
        assert_eq!(item.generation, Some(claim.generation));

        let rendered = report.render().expect("render");
        assert!(rendered.contains("dispatch=cancel_claimed"), "{rendered}");
        assert!(rendered.contains("worker=/root/release_348"), "{rendered}");
        assert!(
            rendered.contains("authoritative cancellation"),
            "{rendered}"
        );
        assert!(
            rendered.contains("never best-effort or sunk-cost"),
            "{rendered}"
        );
        assert!(
            rendered.contains("nearest safe consistency boundary"),
            "{rendered}"
        );
        assert!(
            rendered.contains("stop further implementation, tests, integration"),
            "{rendered}"
        );
        assert!(
            rendered.contains("cannot directly kill a harness subagent"),
            "{rendered}"
        );
        assert!(
            rendered.contains(&format!("--generation {}", claim.generation)),
            "{rendered}"
        );

        // The binary owns the durable state transition but cannot kill the
        // harness worker: the live claim is replaced by an audit tombstone.
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert!(ledger.claims.is_empty(), "{ledger:?}");
        assert_eq!(ledger.cancellations.len(), 1, "{ledger:?}");
        assert_eq!(ledger.cancellations[0].claim.generation, claim.generation);
        let cli = observe(&file, CONSUMER_CLI, true).unwrap().expect("cli");
        assert_eq!(cli.items.len(), 1, "{cli:?}");
        assert_eq!(cli.items[0].dispatch, core::SteeringDispatch::CancelClaimed);
        agent_doc_queue_io::queue_claim::release_attempt(&file, head, Some(claim.generation))
            .unwrap();
        assert!(
            agent_doc_queue_io::queue_claim::load_ledger(&file)
                .unwrap()
                .claims
                .is_empty()
        );
    }

    /// `#queue-removal-cancels`: one removed occurrence cannot authorize
    /// cancellation while an indistinguishable claimed occurrence remains.
    #[test]
    fn removing_one_claimed_duplicate_fails_closed_without_cancelling_worker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let head = "release + publish: include issue 348";
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- {head}\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:release-348", 3600)
                .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &file,
            head,
            "subagent:release-348",
            "/root/release_348",
            3600,
            claim.generation,
        )
        .unwrap();

        let one_left = baseline.replacen(&format!("- {head}\n"), "", 1);
        std::fs::write(&file, &one_left).unwrap();
        backdate(&file);
        let report = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert!(
            report
                .items
                .iter()
                .all(|item| item.dispatch != core::SteeringDispatch::CancelClaimed),
            "{report:?}"
        );
        let stored = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(stored.claims.len(), 1, "{stored:?}");
        assert_eq!(stored.claims[0].generation, claim.generation);
        assert_eq!(
            stored.claims[0]
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str()),
            Some("/root/release_348")
        );
    }

    #[test]
    fn removal_tombstones_reported_generation_and_retires_hidden_completion_outbox() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let head = "finish sdk-demo.md queue fix";
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:exchange patch=append -->\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue go subagents -->\n- current task\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let editor = LiveEditorFixture::new(&file, "cancel-outbox-editor", &baseline);
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:sdk-demo", 3600).unwrap();
        let running = agent_doc_queue_io::queue_claim::start_attempt(
            &file,
            head,
            "subagent:sdk-demo",
            "/root/sdk-demo",
            3600,
            claim.generation,
        )
        .unwrap();
        let reported = agent_doc_queue_io::claim_release_receipt::release_attempt(
            &file,
            head,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Reported,
            Some(claim.generation),
        )
        .unwrap()
        .unwrap();
        assert!(
            agent_doc_queue_io::queue_claim::load_ledger(&file)
                .unwrap()
                .claims
                .is_empty()
        );
        assert_eq!(
            agent_doc_queue_io::claim_release_receipt::load(&file)
                .unwrap()
                .receipts
                .len(),
            1
        );
        let key = agent_doc_queue_io::queue_claim::completion_feedback_key(&reported).unwrap();
        let cell = agent_doc_document::subagent_completion::cell(
            &reported.owner,
            &reported.item_text,
            &key,
        );
        let completion_target =
            agent_doc_element_exchange::insert_prompt_line_before_boundary(&baseline, &cell)
                .unwrap();
        agent_doc_document_realtime_io::retain_deferred_document_write_target(
            &file,
            &baseline,
            &completion_target,
            "queue_subagent_completion_feedback",
            agent_doc_document_realtime_io::DocumentWriteDeferredReason::EditorProjectionPending,
        )
        .unwrap();
        assert!(
            agent_doc_document_realtime_io::pending_document_write(&file)
                .unwrap()
                .target_content
                .contains(&key)
        );

        let deleted = baseline.replace(&format!("- {head}\n"), "");
        let removed_line = format!("- {head}\n");
        agent_doc_op_capture_io::record_editor_op(
            &file,
            &agent_doc_hash::content_hash(&baseline),
            agent_doc_merge::crdt::EditorOp::Delete {
                offset: baseline.find(&removed_line).expect("reported queue row"),
                len: removed_line.len(),
            },
        )
        .unwrap();
        editor.publish(&deleted);
        let report = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert_eq!(
            report.items[0].dispatch,
            core::SteeringDispatch::CancelClaimed
        );
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert!(ledger.claims.is_empty(), "{ledger:?}");
        assert_eq!(ledger.cancellations.len(), 1, "{ledger:?}");
        let tombstone = &ledger.cancellations[0];
        assert_eq!(tombstone.claim.generation, running.generation);
        assert_eq!(tombstone.completion_key.as_deref(), Some(key.as_str()));
        assert!(
            agent_doc_queue_io::claim_release_receipt::load(&file)
                .unwrap()
                .receipts
                .is_empty(),
            "reported receipt must not authorize later integration"
        );
        let successor = agent_doc_document_realtime_io::pending_document_write(&file)
            .expect("cancellation successor");
        assert!(!successor.target_content.contains(&key), "{successor:?}");
        assert!(!successor.target_content.contains(head), "{successor:?}");

        editor.publish(&baseline);
        let (_, replacement) =
            agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:sdk-demo-retry", 3600)
                .unwrap();
        assert_eq!(replacement.generation, running.generation + 1);
        let stale =
            agent_doc_queue_io::queue_claim::report_attempt(&file, head, Some(running.generation))
                .unwrap_err();
        assert!(stale.to_string().contains("stale queue claim generation"));
    }

    /// A live editor's whole-buffer projection can temporarily omit unrelated
    /// reported rows even though its captured operator operations edited only
    /// another part of the document. The projection must not become destructive
    /// cancellation evidence and regress those rows from `☑️` to `📥`.
    #[test]
    fn unrelated_live_editor_op_does_not_cancel_a_reported_queue_row_missing_from_projection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let head = "finish already reported queue work";
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue go subagents -->\n- current task\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let editor = LiveEditorFixture::new(&file, "reported-state-editor", &baseline);
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:reported", 3600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &file,
            head,
            "subagent:reported",
            "/root/reported",
            3600,
            claim.generation,
        )
        .unwrap();
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &file,
            head,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Reported,
            Some(claim.generation),
        )
        .unwrap()
        .expect("reported receipt");

        // The actual operator lineage only appends a note. A stale full-buffer
        // projection loses the reported row at the same time.
        let note = "\noperator is editing another queue row\n";
        agent_doc_op_capture_io::record_editor_op(
            &file,
            &agent_doc_hash::content_hash(&baseline),
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: baseline.len(),
                text: note.to_string(),
            },
        )
        .unwrap();
        let stale_projection = format!("{}{}", baseline.replace(&format!("- {head}\n"), ""), note);
        editor.publish(&stale_projection);
        backdate(&file);
        let report = observe(&file, CONSUMER_HOOK, true).unwrap();
        assert!(
            report.as_ref().is_none_or(|report| report
                .items
                .iter()
                .all(|item| item.dispatch != core::SteeringDispatch::CancelClaimed)),
            "{report:?}"
        );
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert!(ledger.cancellations.is_empty(), "{ledger:?}");
        let receipts = agent_doc_queue_io::claim_release_receipt::load(&file).unwrap();
        assert_eq!(receipts.receipts.len(), 1, "{receipts:?}");
        assert_eq!(receipts.receipts[0].generation, claim.generation);
    }

    #[test]
    fn deleting_terminal_struck_row_is_cleanup_but_live_claim_conflict_is_degraded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let head = "finish worker result";
        let live = format!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &live).unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:worker", 3600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &file,
            head,
            "subagent:worker",
            "/root/worker",
            3600,
            claim.generation,
        )
        .unwrap();
        let struck = live.replace(&format!("- {head}\n"), &format!("- ~{head}~\n"));
        std::fs::write(&file, &struck).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&struck), Some(&struck)).unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &struck,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let deleted = struck.replace(&format!("- ~{head}~\n"), "");
        std::fs::write(&file, &deleted).unwrap();
        backdate(&file);
        let report = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(report.items.len(), 1, "{report:?}");
        assert_eq!(
            report.items[0].dispatch,
            core::SteeringDispatch::DegradedClaimMismatch
        );
        assert_eq!(
            agent_doc_queue_io::queue_claim::load_ledger(&file)
                .unwrap()
                .claims
                .len(),
            1,
            "strikethrough cannot cancel durable live work"
        );

        let clean_file = dir.path().join("clean.md");
        std::fs::write(&clean_file, &struck).unwrap();
        let clean_cycle =
            agent_doc_cycle_state_io::start_preflight(&clean_file, Some(&struck), Some(&struck))
                .unwrap();
        seed_for_cycle(
            &clean_file,
            &clean_cycle.cycle_id,
            &struck,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        std::fs::write(&clean_file, &deleted).unwrap();
        backdate(&clean_file);
        let cleanup = observe(&clean_file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("observation");
        assert!(cleanup.items.is_empty(), "{cleanup:?}");
    }

    /// `#deletedstruckmismatch` (agent-doc-bugs.md, 2026-10-28): a closeout
    /// struck the `subagent:todosync` row, the claim was pruned closed, and no
    /// reported receipt existed, but the completion cell the worker's
    /// `report-complete` wrote stayed in the committed exchange. Queue
    /// maintenance then pruned the struck row, and mid-turn steering read that
    /// binary-owned prune as an operator deletion of owned work on every tool
    /// call. A completion cell with no claim attempt or receipt behind it is
    /// stale residue, never ownership.
    #[test]
    fn pruned_struck_row_with_only_a_stale_completion_cell_is_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let head =
            "`agent:todo` should always be in sync, like how `agent:backlog` is always in sync.";
        let cell =
            agent_doc_document::subagent_completion::cell("subagent:todosync", head, "k-todosync");
        let struck = format!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:exchange -->\nEarlier answer.\n\n{cell}\n<!-- /agent:exchange -->\n\n<!-- agent:queue go -->\n- current task\n- ~~{head}~~\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &struck).unwrap();
        assert_eq!(
            agent_doc_document::subagent_completion::completion_evidence(&struck).len(),
            1,
            "fixture carries the leftover completion cell"
        );
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&struck), Some(&struck)).unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &struck,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let pruned = struck.replace(&format!("- ~~{head}~~\n"), "");
        std::fs::write(&file, &pruned).unwrap();
        backdate(&file);
        let report = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("observation");
        assert!(
            report
                .items
                .iter()
                .all(|item| item.dispatch != core::SteeringDispatch::DegradedClaimMismatch),
            "a closed claim's leftover completion cell must not own the pruned row: {report:?}"
        );
    }

    /// `#deletedstruckmismatch`: the struck row is diffed against the cycle
    /// baseline, which never advances, so every later document change used to
    /// re-derive and re-report the same degraded deletion. A real mismatch is
    /// delivered once per owning attempt; a new attempt reports again.
    #[test]
    fn degraded_struck_row_deletion_is_reported_once_per_attempt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let head = "finish worker result";
        let live = format!(
            "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &live).unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:worker", 3600).unwrap();
        let struck = live.replace(&format!("- {head}\n"), &format!("- ~{head}~\n"));
        std::fs::write(&file, &struck).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&struck), Some(&struck)).unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &struck,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        let degraded = |report: &SteeringReport| {
            report
                .items
                .iter()
                .filter(|item| item.dispatch == core::SteeringDispatch::DegradedClaimMismatch)
                .count()
        };

        let deleted = struck.replace(&format!("- ~{head}~\n"), "");
        std::fs::write(&file, &deleted).unwrap();
        backdate(&file);
        let first = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(degraded(&first), 1, "{first:?}");
        assert_eq!(first.items[0].generation, Some(claim.generation));

        // Unrelated later document changes (the coordinator's own writes, other
        // queue traffic) must not resurface the same mismatch.
        for (round, heading) in ["# S2", "# S3"].into_iter().enumerate() {
            let changed = deleted.replace("# S\n", &format!("{heading}\n"));
            std::fs::write(&file, &changed).unwrap();
            backdate(&file);
            let later = observe(&file, CONSUMER_HOOK, true).unwrap();
            assert!(
                later.as_ref().is_none_or(|report| degraded(report) == 0),
                "round {round}: the same mismatch re-reported: {later:?}"
            );
        }
        assert_eq!(
            agent_doc_queue_io::queue_claim::load_ledger(&file)
                .unwrap()
                .claims
                .len(),
            1,
            "reporting once never fences or cancels the durable claim"
        );

        // A different owning attempt on the same text is a new mismatch.
        let key_a = core::struck_removal_report_key(
            &agent_doc_queue::queue_claim::claim_identity(head),
            Some("subagent:worker"),
            Some(claim.generation),
        );
        let key_b = core::struck_removal_report_key(
            &agent_doc_queue::queue_claim::claim_identity(head),
            Some("subagent:worker"),
            Some(claim.generation + 1),
        );
        assert_ne!(key_a, key_b);
    }

    /// `#nestedsubprompts`: nested rows under a claimed free-text parent are
    /// that parent's sub-prompts. Editing a child, or adding one new child, is
    /// ONE edit of the parent, forwarded to the parent's owner. That child
    /// does not surface as its own item or as new dispatch work, and the
    /// claim stays on the parent. A second direct child added after the claim
    /// is `#nestedclaimsteer` and is covered separately.
    #[test]
    fn nested_subprompt_edit_or_addition_forwards_to_the_parent_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let parent = "Sync PR and Jira state into this document.";
        let head =
            format!("{parent}\n  - So this is the first subtask.\n  - This is the second subtask.");
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 2500\n---\n# S\n\n<!-- agent:run subagents go -->\n- current task\n- {head}\n<!-- /agent:run -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        // The coordinator claims the parent by its parent line alone.
        agent_doc_queue_io::queue_claim::claim(&file, parent, "subagent:pmsync", 3600).unwrap();

        for (label, edited) in [
            (
                "child edited",
                baseline.replace("second subtask.", "second subtask, now steered."),
            ),
            (
                "child added",
                baseline.replace(
                    "  - This is the second subtask.\n",
                    "  - This is the second subtask.\n  - A brand new third subtask.\n",
                ),
            ),
        ] {
            std::fs::write(&file, &edited).unwrap();
            backdate(&file);
            let hook = observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .unwrap_or_else(|| panic!("{label}: no steering observation"));
            assert_eq!(
                hook.items.len(),
                1,
                "{label}: one parent edit, never per-child items: {hook:?}"
            );
            let item = &hook.items[0];
            assert_eq!(
                item.change,
                core::SteeringChange::Edited,
                "{label}: {hook:?}"
            );
            assert_eq!(
                item.dispatch,
                core::SteeringDispatch::ForwardToOwner,
                "{label}: {hook:?}"
            );
            assert_eq!(item.owner.as_deref(), Some("subagent:pmsync"), "{label}");
            assert!(
                item.verbatim.starts_with(parent),
                "{label}: {}",
                item.verbatim
            );
            let claimed =
                agent_doc_queue_io::queue_claim::claimed_items_for_content(&file, &edited);
            assert!(
                claimed.claims(&item.verbatim),
                "{label}: the claim stays on the parent"
            );
            // Reset to the baseline between cases so each is a single edit.
            std::fs::write(&file, &baseline).unwrap();
            backdate(&file);
            let _ = observe(&file, CONSUMER_HOOK, true).unwrap();
        }
    }

    /// `#nestedclaimsteer`: the first direct child added after a claim steers
    /// that claim. Each later direct child is its own head, at its authored
    /// indent, and may route to another subagent with the parent's lineage.
    /// The same split applies at every depth once that node is claimed.
    #[test]
    fn later_nested_children_route_with_lineage_at_every_depth() {
        let parent = "Sync PR and Jira state into this document.";
        let first = "First steer child stays with the parent.";
        let second = "Second route child gets its own worker.";
        for marker in ["run", "queue", "loop"] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            let file = dir.path().join("plan.md");
            let baseline = format!(
                "---\nagent_doc_steering_debounce_ms: 2500\n---\n# S\n\n<!-- agent:{marker} subagents go -->\n- current task\n- {parent}\n<!-- /agent:{marker} -->\n"
            );
            std::fs::write(&file, &baseline).unwrap();
            let cycle =
                agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                    .unwrap();
            seed_for_cycle(
                &file,
                &cycle.cycle_id,
                &baseline,
                Some("current task"),
                Vec::new(),
            )
            .unwrap();
            agent_doc_queue_io::queue_claim::claim(&file, parent, "subagent:pmsync", 3600).unwrap();
            let edited = baseline.replace(
                &format!("- {parent}\n"),
                &format!("- {parent}\n  - {first}\n  - {second}\n"),
            );
            std::fs::write(&file, &edited).unwrap();
            backdate(&file);
            let hook = observe(&file, CONSUMER_HOOK, true)
                .unwrap()
                .unwrap_or_else(|| panic!("{marker}: no steering observation"));
            let steered = hook
                .items
                .iter()
                .find(|item| item.change == core::SteeringChange::Edited)
                .unwrap_or_else(|| panic!("{marker}: missing parent edit: {hook:?}"));
            assert_eq!(
                steered.dispatch,
                core::SteeringDispatch::ForwardToOwner,
                "{marker}: {hook:?}"
            );
            assert_eq!(
                steered.owner.as_deref(),
                Some("subagent:pmsync"),
                "{marker}"
            );
            assert!(
                steered.verbatim.starts_with(parent),
                "{marker}: {}",
                steered.verbatim
            );
            assert!(
                steered.verbatim.contains(first),
                "{marker}: {}",
                steered.verbatim
            );
            assert!(
                !steered.verbatim.contains(second),
                "{marker}: {}",
                steered.verbatim
            );
            let routed = hook
                .items
                .iter()
                .find(|item| item.verbatim.contains(second))
                .unwrap_or_else(|| panic!("{marker}: second child missing: {hook:?}"));
            assert_eq!(
                routed.change,
                core::SteeringChange::Added,
                "{marker}: {hook:?}"
            );
            assert_eq!(
                routed.dispatch,
                core::SteeringDispatch::Subagent,
                "{marker}: {hook:?}"
            );
            assert!(edited.contains(&format!("\n  - {second}\n")), "{marker}");
            assert!(!edited.contains(&format!("\n- {second}\n")), "{marker}");
            let claimed =
                agent_doc_queue_io::queue_claim::claimed_items_for_content(&file, &edited);
            assert!(claimed.claims(parent), "{marker}");
            assert!(claimed.claims(&steered.verbatim), "{marker}");
            assert!(!claimed.claims(second), "{marker}");
            let dispatch =
                agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_for_content(
                    &file, &edited,
                )
                .unwrap();
            let lineage = format!("{parent}: {second}");
            assert!(
                dispatch.iter().any(|head| head == &lineage),
                "{marker}: a unique nested leaf is listed as its nest vector (`#promptsurface`): {dispatch:?}"
            );
            assert!(
                dispatch
                    .iter()
                    .all(|head| !head.contains(first) && head != parent),
                "{marker}: {dispatch:?}"
            );
            let auth = agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_content(
                &file, &edited, second,
            )
            .unwrap();
            let preamble = &auth.subagent_prompt_preamble;
            assert!(
                preamble.contains(&format!("> {parent}: {second}")),
                "{marker}: {preamble}"
            );
            assert!(
                preamble.contains("Prompt tree (`#nestsubmit`"),
                "{marker}: {preamble}"
            );
        }

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 2500\n---\n# S\n\n<!-- agent:run subagents go -->\n- current task\n- {parent}\n<!-- /agent:run -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&file, parent, "subagent:pmsync", 3600).unwrap();
        let with_children = baseline.replace(
            &format!("- {parent}\n"),
            &format!("- {parent}\n  - {first}\n  - {second}\n"),
        );
        std::fs::write(&file, &with_children).unwrap();
        backdate(&file);
        observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("two children");
        agent_doc_queue_io::queue_claim::claim(&file, second, "subagent:route", 3600)
            .expect("routed sibling is its own claim target");
        let first_gc = "First grandchild steers the routed child.";
        let second_gc = "Second grandchild routes one level deeper.";
        let with_grandchildren = with_children.replace(
            &format!("  - {second}\n"),
            &format!("  - {second}\n    - {first_gc}\n    - {second_gc}\n"),
        );
        std::fs::write(&file, &with_grandchildren).unwrap();
        backdate(&file);
        let hook = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("grandchild steering");
        let steered = hook
            .items
            .iter()
            .find(|item| item.dispatch == core::SteeringDispatch::ForwardToOwner)
            .unwrap_or_else(|| panic!("grandchild steer missing: {hook:?}"));
        assert!(steered.verbatim.contains(second), "{}", steered.verbatim);
        assert!(steered.verbatim.contains(first_gc), "{}", steered.verbatim);
        assert!(
            !steered.verbatim.contains(second_gc),
            "{}",
            steered.verbatim
        );
        assert_eq!(steered.owner.as_deref(), Some("subagent:route"));
        let routed = hook
            .items
            .iter()
            .find(|item| item.verbatim.contains(second_gc))
            .unwrap_or_else(|| panic!("deeper head missing: {hook:?}"));
        assert_eq!(routed.change, core::SteeringChange::Added);
        assert_eq!(routed.dispatch, core::SteeringDispatch::Subagent);
        assert!(with_grandchildren.contains(&format!("\n    - {second_gc}\n")));
        assert!(!with_grandchildren.contains(&format!("\n- {second_gc}\n")));
        let auth = agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_content(
            &file,
            &with_grandchildren,
            second_gc,
        )
        .unwrap();
        let preamble = &auth.subagent_prompt_preamble;
        assert!(
            preamble.contains(&format!("> {parent}: {second}: {second_gc}")),
            "{preamble}"
        );
        assert!(
            preamble.contains("Prompt tree (`#nestsubmit`"),
            "{preamble}"
        );
        let claims =
            agent_doc_queue_io::queue_claim::active_claims_for_content(&file, &with_grandchildren)
                .unwrap();
        assert!(
            claims.iter().any(|claim| claim.owner == "subagent:route"),
            "{claims:?}"
        );
        agent_doc_queue_io::queue_claim::prune_closed_claims(&file, &with_grandchildren).unwrap();
        let stored = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert!(
            stored
                .claims
                .iter()
                .any(|claim| claim.owner == "subagent:route"),
            "prune keeps the routed claim: {stored:?}"
        );
    }

    /// `#steerworks` items 1 + 3 (agent-doc-bugs.md, 2026-10-04): the operator
    /// annotates a CLAIMED `#gh-fix` head while the coordinator is idle. The
    /// claim follows the edit, the idle wake fires for it, the woken cycle's
    /// seed keeps it pending, and its first hook says "forward to owner", not
    /// "dispatch a new subagent". Further edits route the same way with no
    /// re-claim; a complete rewrite into a different request is a replacement
    /// that cancels the owner instead (`#krwr`).
    #[test]
    fn claimed_head_edit_wakes_the_idle_coordinator_and_forwards_to_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let url = "https://github.com/btakita/agent-doc/issues/126";
        let head = format!("#subagents: #gh-fix {url}");
        let baseline = format!(
            "---\nagent_doc_steering_debounce_ms: 2500\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- {head}\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&file, &baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            &baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&file, &head, "subagent:ghfix126", 3600).unwrap();
        let closed = baseline.replace("- current task\n", "");
        std::fs::write(&file, &closed).unwrap();
        close_cycle(&file, &closed);
        let mut report = Vec::new();
        emit_closeout_steering(&file, &mut report);
        assert!(observe_for_wake(&file).unwrap().items.is_empty());

        // Idle: the operator annotates the claimed head.
        let annotated_head = format!("{head}: note this is in a Coder environment");
        let annotated = closed.replace(&format!("- {head}\n"), &format!("- {annotated_head}\n"));
        std::fs::write(&file, &annotated).unwrap();
        backdate(&file);
        let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(&file, &annotated);
        assert!(
            claimed.claims(&annotated_head),
            "the claim followed the edit"
        );
        let wake = observe_for_wake(&file).unwrap();
        assert_eq!(
            wake.items.len(),
            1,
            "the idle coordinator is woken: {wake:?}"
        );
        assert_eq!(
            wake.items[0].dispatch,
            core::SteeringDispatch::ForwardToOwner
        );
        assert_eq!(wake.items[0].owner.as_deref(), Some("subagent:ghfix126"));

        // The woken cycle: preflight seeds a NEW cycle from the annotated text.
        let woken =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&annotated), Some(&annotated))
                .unwrap();
        seed_for_cycle(&file, &woken.cycle_id, &annotated, None, Vec::new()).unwrap();
        let hook = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("first hook");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        let text = hook.render().unwrap();
        assert!(
            text.contains(
                "dispatch=forward_to_owner source=queue change=edited owner=subagent:ghfix126"
            ),
            "{text}"
        );
        assert!(text.contains(&format!("previous: {head}")), "{text}");
        assert!(!text.contains("dispatch=subagent"), "{text}");

        // A trailing period: still the owner's work, no re-claim needed.
        let period = annotated.replace(&annotated_head, &format!("{annotated_head}."));
        std::fs::write(&file, &period).unwrap();
        backdate(&file);
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_eq!(hook.items.len(), 1, "{hook:?}");
        assert_eq!(
            hook.items[0].dispatch,
            core::SteeringDispatch::ForwardToOwner
        );
        let ledger =
            agent_doc_queue_io::queue_claim::load_ledger_following(&file, &period).unwrap();
        assert_eq!(ledger.claims.len(), 1, "{ledger:?}");
        assert_eq!(ledger.claims[0].owner, "subagent:ghfix126");
        // Closeout pruning keeps (and persists) the followed claim.
        agent_doc_queue_io::queue_claim::prune_closed_claims(&file, &period).unwrap();
        let stored = agent_doc_queue_io::queue_claim::load_ledger(&file).unwrap();
        assert_eq!(stored.claims.len(), 1, "{stored:?}");
        assert_eq!(stored.claims[0].item_text, format!("{annotated_head}."));

        // `#krwr`: replacing the row with a different request in its slot is
        // a deletion plus new work, not an edit: the owner is cancelled and
        // the new row is unclaimed, ordinary queue work.
        let replacement = "replace this claimed work with a wholly different request";
        let rewritten = period.replace(&format!("{annotated_head}."), replacement);
        std::fs::write(&file, &rewritten).unwrap();
        backdate(&file);
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
        assert_replacement_cancels(&file, &hook, &rewritten, replacement, "subagent:ghfix126");
    }

    /// `#krwr` assertions shared by the replacement tests: the replaced row's
    /// owner gets `cancel_claimed`, the new row is unowned queue work, and the
    /// ledger tombstones the attempt instead of following it.
    fn assert_replacement_cancels(
        file: &Path,
        report: &SteeringReport,
        content: &str,
        replacement: &str,
        owner: &str,
    ) {
        assert!(
            report
                .items
                .iter()
                .all(|item| item.dispatch != core::SteeringDispatch::ForwardToOwner),
            "{report:?}"
        );
        let cancelled: Vec<_> = report
            .items
            .iter()
            .filter(|item| item.dispatch == core::SteeringDispatch::CancelClaimed)
            .collect();
        assert_eq!(cancelled.len(), 1, "{report:?}");
        assert_eq!(cancelled[0].change, core::SteeringChange::Deleted);
        assert_eq!(cancelled[0].owner.as_deref(), Some(owner));
        assert_ne!(cancelled[0].verbatim, replacement);
        let added: Vec<_> = report
            .items
            .iter()
            .filter(|item| item.verbatim == replacement)
            .collect();
        assert_eq!(added.len(), 1, "{report:?}");
        assert_eq!(added[0].change, core::SteeringChange::Added);
        assert_eq!(added[0].owner, None, "{report:?}");
        assert_eq!(added[0].previous, None, "{report:?}");
        let rendered = report.render().expect("render");
        assert!(rendered.contains("dispatch=cancel_claimed"), "{rendered}");
        assert!(
            !rendered.contains("dispatch=forward_to_owner"),
            "{rendered}"
        );

        let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(file, content);
        assert!(
            !claimed.claims(replacement),
            "the new row is unclaimed and dispatchable"
        );
        let stored = agent_doc_queue_io::queue_claim::load_ledger(file).unwrap();
        assert!(stored.claims.is_empty(), "{stored:?}");
        assert_eq!(stored.cancellations.len(), 1, "{stored:?}");
        assert_eq!(stored.cancellations[0].claim.owner, owner);
    }

    /// `#krwr`: deleting a claimed `do [#a]` row while adding `do [#b]` in
    /// the same slot cancels `#a`'s worker; `do [#b]` is new work. Rewriting
    /// the prose of a row that keeps its tracked id still steers the owner.
    #[test]
    fn replacing_a_claimed_id_row_in_its_slot_cancels_but_a_same_id_rewrite_forwards() {
        for (after, replaced) in [
            ("do [#b]", true),
            ("do [#a]: actually ship it as a migration instead", false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            let file = dir.path().join("plan.md");
            let head = "do [#a]";
            let baseline = format!(
                "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n- {head}\n- later work\n<!-- /agent:queue -->\n"
            );
            std::fs::write(&file, &baseline).unwrap();
            let cycle =
                agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                    .unwrap();
            seed_for_cycle(
                &file,
                &cycle.cycle_id,
                &baseline,
                Some("current task"),
                Vec::new(),
            )
            .unwrap();
            let (_, claim) =
                agent_doc_queue_io::queue_claim::claim(&file, head, "subagent:a", 3600).unwrap();
            agent_doc_queue_io::queue_claim::start_attempt(
                &file,
                head,
                "subagent:a",
                "/root/a",
                3600,
                claim.generation,
            )
            .unwrap();

            let edited = baseline.replace(&format!("- {head}\n"), &format!("- {after}\n"));
            std::fs::write(&file, &edited).unwrap();
            backdate(&file);
            let report = observe(&file, CONSUMER_HOOK, true).unwrap().expect("hook");
            if replaced {
                assert_replacement_cancels(&file, &report, &edited, after, "subagent:a");
                let cancelled = report
                    .items
                    .iter()
                    .find(|item| item.dispatch == core::SteeringDispatch::CancelClaimed)
                    .unwrap();
                assert_eq!(cancelled.verbatim, head);
                assert_eq!(cancelled.worker_id.as_deref(), Some("/root/a"));
                assert_eq!(cancelled.generation, Some(claim.generation));
            } else {
                assert_eq!(report.items.len(), 1, "{report:?}");
                let item = &report.items[0];
                assert_eq!(item.change, core::SteeringChange::Edited);
                assert_eq!(item.dispatch, core::SteeringDispatch::ForwardToOwner);
                assert_eq!(item.owner.as_deref(), Some("subagent:a"));
                assert_eq!(item.generation, Some(claim.generation));
                assert_eq!(item.previous.as_deref(), Some(head));
                assert_eq!(item.verbatim, after);
                let stored =
                    agent_doc_queue_io::queue_claim::load_ledger_following(&file, &edited).unwrap();
                assert_eq!(stored.claims.len(), 1, "{stored:?}");
                assert!(stored.cancellations.is_empty(), "{stored:?}");
                assert!(
                    agent_doc_queue_io::queue_claim::claimed_items_for_content(&file, &edited)
                        .claims(after)
                );
            }
        }
    }

    /// `#steerbaselineabsorb` (agent-doc-bugs.md, 2026-10-04): the operator
    /// finished a prompt line while `respond` was committing, the commit
    /// absorbed it, and the steering hook kept re-surfacing the same item after
    /// a later cycle had answered and committed it. Once the absorbed prompt is
    /// answered, no steering consumer lists it again.
    #[test]
    fn answered_absorbed_steering_is_never_resurfaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let prompt = "testing to see if you pick this up.";
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:exchange -->\nEarlier prompt.\n\n### Re: Earlier prompt\n\nAnswer.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, baseline).unwrap();
        let first =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(&file, &first.cycle_id, baseline, None, Vec::new()).unwrap();

        // The closeout commit absorbs the operator's line into its baseline.
        let absorbed = baseline.replace("Answer.\n", &format!("Answer.\n\n{prompt}\n"));
        std::fs::write(&file, &absorbed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &absorbed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        close_cycle(&file, &absorbed);
        backdate(&file);

        // Control: the hook's stale cycle base still derives the item.
        let peek = observe(&file, CONSUMER_HOOK, false)
            .unwrap()
            .expect("hook peek");
        assert_eq!(peek.items.len(), 1, "{peek:?}");
        assert_eq!(peek.items[0].verbatim, prompt);
        assert_eq!(peek.items[0].dispatch, core::SteeringDispatch::AddressNow);
        // The `steering pending` report persists it: the committed baseline
        // carries the text, so the next preflight would otherwise diff clean.
        assert_eq!(
            crate::absorbed_steering::record_reported(&file, &peek.items, &first.cycle_id).unwrap(),
            vec![prompt.to_string()]
        );
        assert_eq!(
            crate::absorbed_steering::unanswered_prompts(&file).unwrap(),
            vec![prompt.to_string()]
        );

        // A later response cycle answers it and commits without a preflight
        // re-seed (a `respond` that reopens a committed cycle from HEAD).
        let second =
            agent_doc_cycle_state_io::start_preflight(&file, Some(&absorbed), Some(&absorbed))
                .unwrap();
        assert_ne!(second.cycle_id, first.cycle_id);
        let answered = absorbed.replace(
            "<!-- /agent:exchange -->",
            "### Re: testing to see — opus\n\nYes, picked up.\n<!-- /agent:exchange -->",
        );
        std::fs::write(&file, &answered).unwrap();
        capture_response_body(
            &file,
            "<!-- patch:exchange -->\n### Re: testing to see — opus\n\nYes, picked up.\n<!-- /patch:exchange -->\n",
            &absorbed,
        );
        close_cycle(&file, &answered);
        backdate(&file);

        assert!(
            crate::absorbed_steering::unanswered_prompts(&file)
                .unwrap()
                .is_empty()
        );
        let hook = observe(&file, CONSUMER_HOOK, true).unwrap();
        assert!(
            hook.as_ref().is_none_or(|report| report.items.is_empty()),
            "{hook:?}"
        );
        assert!(observe_for_wake(&file).unwrap().items.is_empty());
        assert!(pending_steering_items(&file, &answered).is_empty());
    }
}

//! Durable queue-item claims (`#queueclaim`).
//!
//! Storage for [`agent_doc_queue::queue_claim`]: one `queue_document_state` row
//! per document (`state_kind = "queue_claims"`) in the project `state.db`,
//! holding the document's [`QueueClaimLedger`] as JSON. Claims are
//! document-scoped rather than cycle-scoped on purpose: a subagent outlives the
//! cycle that dispatched it, and the parent turn may close several cycles while
//! the worker is still running.
//!
//! The readers here are consulted by actorless one-shot boundaries (the Stop
//! hooks, `session-check`, preflight's queue projection), the same seams that
//! already read the continuation request and marker from `state.db`. Nothing
//! here arbitrates a live actor transition; a claim only removes a head from
//! the in-session loop's drainable set.
//!
//! Read-modify-write runs inside `BEGIN IMMEDIATE`, so two subagents claiming
//! different heads at once cannot lose either claim.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_queue::queue_claim::{
    ClaimFollowOutcome, ClaimHeadSource, ClaimOutcome, ClaimOwnerLiveness, ClaimOwnerSession,
    ClaimQueueView, ClaimedQueueItems, OwnerSessionObservation, QueueClaim,
    QueueClaimCancellationReceipt, QueueClaimLedger, QueueClaimMiss, QueueClaimRefreshRefused,
    claim_identity, nest_leaf_matches, owner_session_liveness, resolve_claim_target_in_views,
};
use agent_doc_queue::queue_continuation::{
    all_executable_row_keys, all_queue_head_texts, live_queue_head_identities,
    live_queue_head_texts,
};
use anyhow::{Context, Result, bail};

use agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND;

/// Debug-build test seam (`#finalgrace`): an `agent-doc` process built with
/// debug assertions reads the claim-ledger clock from this variable when it is
/// set to whole epoch seconds. An integration test pins it on every CLI command
/// it spawns, so claim timestamps, TTL expiry, and the
/// [`DISPATCH_EVIDENCE_GRACE_SECS`](agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS)
/// age of an unacknowledged claim do not depend on how fast the test ran.
/// Optimized (release) builds ignore it.
pub const TEST_CLAIM_CLOCK_ENV: &str = "AGENT_DOC_TEST_QUEUE_CLAIM_NOW_SECS";

thread_local! {
    static PINNED_CLAIM_CLOCK: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Restores the previous thread pin when dropped; see
/// [`pin_claim_clock_for_current_thread`].
#[must_use = "the claim clock is pinned only while the guard is alive"]
pub struct PinnedClaimClock {
    previous: Option<u64>,
}

impl Drop for PinnedClaimClock {
    fn drop(&mut self) {
        PINNED_CLAIM_CLOCK.with(|pinned| pinned.set(self.previous));
    }
}

/// In-process form of [`TEST_CLAIM_CLOCK_ENV`] for unit tests (`#finalgrace`).
/// Process environment is shared by every test thread, so an in-process test
/// pins the claim clock for its own thread instead. Debug builds only.
#[doc(hidden)]
pub fn pin_claim_clock_for_current_thread(now_secs: u64) -> PinnedClaimClock {
    PinnedClaimClock {
        previous: PINNED_CLAIM_CLOCK.with(|pinned| pinned.replace(Some(now_secs))),
    }
}

/// The clock every claim-ledger timestamp and dispatch-acknowledgement age is
/// measured on. Production reads the system clock; a debug build honours a
/// thread pin, then [`TEST_CLAIM_CLOCK_ENV`].
pub fn claim_clock_now_secs() -> u64 {
    if cfg!(debug_assertions) {
        if let Some(pinned) = PINNED_CLAIM_CLOCK.with(std::cell::Cell::get) {
            return pinned;
        }
        if let Some(pinned) = std::env::var(TEST_CLAIM_CLOCK_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
        {
            return pinned;
        }
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    claim_clock_now_secs()
}

/// The harness session the current process runs in (`#claimownerliveness`),
/// from its environment. A subagent inherits its coordinator's variables, so a
/// claim a subagent makes binds to the coordinator session. `None` outside any
/// harness.
pub fn current_owner_session() -> Option<ClaimOwnerSession> {
    owner_session_from_env(|key| std::env::var(key).ok())
}

fn owner_session_from_env(var: impl Fn(&str) -> Option<String>) -> Option<ClaimOwnerSession> {
    let present = |key: &str| {
        var(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let (harness, session_id) = if let Some(id) = present("CLAUDE_CODE_SESSION_ID") {
        (Some("claude".to_string()), Some(id))
    } else if let Some(id) = present("CODEX_THREAD_ID") {
        (Some("codex".to_string()), Some(id))
    } else {
        (None, None)
    };
    ClaimOwnerSession {
        harness,
        session_id,
        pane: present("TMUX_PANE"),
        pid: present("CLAUDE_PID").and_then(|pid| pid.parse().ok()),
    }
    .non_empty()
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // Signal 0 probes existence without delivering anything; EPERM still
    // means the process exists.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Without a signal-0 probe the owner pid cannot be judged, so it is treated as
/// alive: the claim keeps its pre-liveness "in flight" reading rather than being
/// reported stale on evidence this platform cannot produce.
#[cfg(not(unix))]
fn pid_alive(pid: u32) -> bool {
    pid != 0
}

/// Judge `claim`'s owner session from this process (`#claimownerliveness`):
/// the caller's own harness session, whether the recorded pid is alive, and
/// the session id the claim's pane last reported through a turn hook.
pub fn claim_owner_liveness(file: &Path, claim: &QueueClaim) -> ClaimOwnerLiveness {
    let caller = current_owner_session();
    let pane_session = claim
        .owner_session
        .as_ref()
        .and_then(|owner| owner.pane.as_deref())
        .and_then(|pane| agent_doc_turn_status_io::pane_harness_session_for_file(file, pane));
    claim_owner_liveness_observed(claim, caller.as_ref(), pane_session, pid_alive)
}

fn claim_owner_liveness_observed(
    claim: &QueueClaim,
    caller: Option<&ClaimOwnerSession>,
    pane_session: Option<(String, u64)>,
    probe_pid: impl Fn(u32) -> bool,
) -> ClaimOwnerLiveness {
    let owner_pid_alive = claim
        .owner_session
        .as_ref()
        .and_then(|owner| owner.pid)
        .map(probe_pid);
    owner_session_liveness(
        claim,
        OwnerSessionObservation {
            caller,
            owner_pid_alive,
            pane_session: pane_session
                .as_ref()
                .map(|(session, at)| (session.as_str(), *at)),
        },
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

fn load_from_conn(
    conn: &agent_doc_sqlite::state_store::Connection,
    document_hash: &str,
) -> Result<QueueClaimLedger> {
    let Some(record) = agent_doc_sqlite::state_store::load_queue_document_state_from_db(
        conn,
        document_hash,
        QUEUE_CLAIMS_STATE_KIND,
    )?
    else {
        return Ok(QueueClaimLedger::default());
    };
    // `#claimdispatchidentity`: the decode re-keys claims stored under an older
    // identity rule; the next mutation persists the re-key.
    QueueClaimLedger::from_state_payload(&record.payload_json)
}

/// Apply `mutate` to the document's ledger in one immediate transaction.
fn mutate_ledger<T>(
    file: &Path,
    mutate: impl FnOnce(&mut QueueClaimLedger) -> Result<T>,
) -> Result<T> {
    let Some((root, document_hash, canonical_path)) = state_identity(file)? else {
        bail!(
            "{} is not inside an agent-doc project (no .agent-doc root); queue claims need state.db",
            file.display()
        );
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .context("begin queue claim transaction")?;
    let result = (|| {
        let mut ledger = load_from_conn(&conn, &document_hash)?;
        // `#claimfollowsedit`: persist claims that followed an operator
        // annotation of their head before the mutation looks them up.
        if !ledger.claims.is_empty()
            && let Some(view) = std::fs::read_to_string(file)
                .ok()
                .as_deref()
                .and_then(ClaimQueueView::from_content)
        {
            log_followed(file, &ledger.follow_edits_in_view(now_secs(), &view));
        }
        let value = mutate(&mut ledger)?;
        if ledger.claims.is_empty() && ledger.cancellations.is_empty() {
            agent_doc_sqlite::state_store::clear_queue_document_state_in_db(
                &conn,
                &document_hash,
                QUEUE_CLAIMS_STATE_KIND,
            )?;
        } else {
            agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
                &conn,
                &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                    document_hash: document_hash.clone(),
                    state_kind: QUEUE_CLAIMS_STATE_KIND.to_string(),
                    canonical_path,
                    payload_json: serde_json::to_string(&ledger)
                        .context("serialize queue claim ledger")?,
                    updated_at_secs: now_secs(),
                },
            )?;
        }
        Ok(value)
    })();
    match result {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .context("commit queue claim transaction")?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback) = conn.execute_batch("ROLLBACK") {
                eprintln!("[queue-claim] WARNING: rollback failed after {err:#}: {rollback}");
            }
            Err(err)
        }
    }
}

fn log_followed(file: &Path, outcome: &ClaimFollowOutcome) {
    for (previous, current) in &outcome.followed {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_followed_edit previous_bytes={} current_bytes={} (#claimfollowsedit)",
                previous.len(),
                current.len()
            ),
        );
    }
    // `#claimfollowstruck`: a claim that had followed off a struck row onto
    // unrelated live work is restored onto its struck row, where the strike
    // policy judges it (retire a finished attempt, `❔` for a running one).
    for (held, restored) in &outcome.unfollowed_struck {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_unfollowed_struck held_bytes={} restored_bytes={} (#claimfollowstruck)",
                held.len(),
                restored.len()
            ),
        );
    }
}

/// The ledger as of `content`: claims carried across operator annotations
/// of their heads (`#claimfollowsedit`). Read-only; the next mutation
/// persists the follow.
pub fn load_ledger_following(file: &Path, content: &str) -> Result<QueueClaimLedger> {
    let mut ledger = load_ledger(file)?;
    if !ledger.claims.is_empty()
        && let Some(view) = ClaimQueueView::from_content(content)
    {
        ledger.follow_edits_in_view(now_secs(), &view);
    }
    Ok(ledger)
}

/// Compatibility baselines for an older folded `#nestedclaimsteer` entry.
/// Canonical parsed trees are already leaf-only.
pub fn nested_claim_steer_snapshots(
    file: &Path,
    content: &str,
) -> Vec<agent_doc_queue::nested_claim_steer::ClaimTimeSnapshot> {
    let Ok(ledger) = load_ledger_following(file, content) else {
        return Vec::new();
    };
    snapshots_from_ledger(&ledger)
}

fn snapshots_from_ledger(
    ledger: &QueueClaimLedger,
) -> Vec<agent_doc_queue::nested_claim_steer::ClaimTimeSnapshot> {
    let now = now_secs();
    ledger
        .claims
        .iter()
        .filter(|claim| claim.blocks_dispatch(now))
        .map(agent_doc_queue::nested_claim_steer::ClaimTimeSnapshot::from_claim)
        .collect()
}

/// Marker-stripped text of the expanded row whose lineage key is `target`.
///
/// `None` when that row is already `target` (a top-level head, or a tracked
/// directive). A nested leaf's key is the nest vector, so the returned text is
/// the authored row (`#nestlistlineage`).
fn nested_claim_row_at_claim(file: &Path, target: &str) -> Option<String> {
    let content = std::fs::read_to_string(file).ok()?;
    let _guard = steer_guard(nested_claim_steer_snapshots(file, &content));
    all_executable_row_keys(&content)?
        .into_iter()
        .find_map(|(display, lineage)| (lineage == target && display != target).then_some(display))
}

fn steer_guard(
    snapshots: Vec<agent_doc_queue::nested_claim_steer::ClaimTimeSnapshot>,
) -> Option<agent_doc_queue::nested_claim_steer::NestedClaimSteerGuard> {
    agent_doc_queue::nested_claim_steer::NestedClaimSteerGuard::install_if_nonempty(snapshots)
}

pub fn steer_guard_for(
    file: &Path,
    content: &str,
) -> Option<agent_doc_queue::nested_claim_steer::NestedClaimSteerGuard> {
    steer_guard(nested_claim_steer_snapshots(file, content))
}

/// [`steer_guard_for`] under a name the preflight hot path can call.
///
/// `agent-doc-preflight-io` budgets the `guard_` token. This alias keeps that
/// source free of it.
pub fn nested_claim_steer_for(
    file: &Path,
    content: &str,
) -> Option<agent_doc_queue::nested_claim_steer::NestedClaimSteerGuard> {
    steer_guard_for(file, content)
}

/// Owner of every active claim on a live head of `content`, by identity
/// (`#claimedsteerwake`). An unreadable ledger is reported and empty.
pub fn claim_owners_for_content(
    file: &Path,
    content: &str,
) -> std::collections::HashMap<agent_doc_element_queue::QueueItemIdentity, String> {
    match load_ledger_following(file, content) {
        Ok(ledger) if ledger.claims.is_empty() => Default::default(),
        Ok(ledger) => {
            let _guard = steer_guard(snapshots_from_ledger(&ledger));
            let live = live_queue_head_identities(content);
            ledger.owners(now_secs(), live.as_ref())
        }
        Err(err) => {
            eprintln!(
                "[queue-claim] WARNING: could not read queue claims for {}: {err:#}",
                file.display()
            );
            Default::default()
        }
    }
}

/// The document's stored claim ledger, including expired/closed rows.
pub fn load_ledger(file: &Path) -> Result<QueueClaimLedger> {
    let Some((root, document_hash, _)) = state_identity(file)? else {
        return Ok(QueueClaimLedger::default());
    };
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    load_from_conn(&conn, &document_hash)
}

/// Work texts of projection-bearing queue-head rows.
fn row_head_texts(rows: &[String]) -> Vec<String> {
    rows.iter()
        .map(|row| agent_doc_document::queue_projection::strip_in_progress_marker(row))
        .collect()
}

/// The editor/CRDT authority's current text of `file`. `None` when no editor
/// owns the document (disk is the authority) or no current cut is available.
pub(crate) fn editor_authority_text(file: &Path, source: &str) -> Option<String> {
    match agent_doc_controller_io::project_controller::current_text_via_controller_model_read_for_doc(
        file, source,
    ) {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => Some(text),
        _ => None,
    }
}

/// Compatibility fallback for an older folded `#nestedclaimsteer` view.
fn resolve_expanded_claim_target(
    file: &Path,
    item: &str,
    content: &str,
    authority_text: Option<&str>,
) -> Option<(String, agent_doc_queue::queue_claim::ClaimHeadSource)> {
    let _guard = steer_guard(nested_claim_steer_snapshots(file, content))?;
    let disk_heads = all_queue_head_texts(content).unwrap_or_default();
    let authority_heads = authority_text.and_then(all_queue_head_texts);
    agent_doc_queue::queue_claim::resolve_claim_target_in_views(
        item,
        &disk_heads,
        authority_heads.as_deref(),
    )
    .ok()
}

/// Resolve a claim `--item` against the live editor authority when available,
/// otherwise against the on-disk document (GH #124 ask 5, GH #296): dispatch
/// and claim must agree on what a live head is even while an editor save to
/// disk is deferred. Returns the resolved head and every queue of the view it
/// resolved in (a new claim's anchor, `#claimfollowedit`; per queue, so a
/// claim never aligns with a sibling queue, `#claimfollowstruck`), with their
/// lifecycle projection retained as row evidence (`#claimrowmigrate`).
///
/// An item that names no live head but names a claim that has since followed
/// an operator edit of its line resolves to the line it became
/// (`#claimfollowedit`), so a worker's `--refresh` / `release` by the text it
/// was handed still reaches its claim.
fn resolve_live_claim_target(
    file: &Path,
    item: &str,
    source: &str,
) -> Result<(String, ClaimQueueView)> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("read {} to resolve queue item {item:?}", file.display()))?;
    let disk_view = ClaimQueueView::from_content(&content).unwrap_or_default();
    // Claim keys are nest vectors for a nested non-tracked leaf
    // (`#nestlistlineage`), from every queue. Display rows stay on the anchor
    // via `disk_view`.
    let disk_heads = all_queue_head_texts(&content).unwrap_or_default();
    // The editor/CRDT authority's current text, the view preflight computes
    // `queue_subagent_dispatch` from. `None` when no editor owns the document
    // (disk is the authority) or no current cut is available.
    let authority_text = editor_authority_text(file, source);
    let authority_heads = authority_text.as_deref().and_then(all_queue_head_texts);
    let authority_view = authority_heads.as_ref().map(|_| {
        authority_text
            .as_deref()
            .and_then(ClaimQueueView::from_content)
            .unwrap_or_default()
    });
    let resolved = resolve_claim_target_in_views(item, &disk_heads, authority_heads.as_deref());
    let (target, view) = match resolved {
        Ok(resolved) => resolved,
        Err(miss) if miss.ambiguous.is_empty() => {
            let expanded =
                resolve_expanded_claim_target(file, item, &content, authority_text.as_deref());
            if let Some(resolved) = expanded {
                resolved
            } else {
                let authority = authority_heads.as_deref().zip(authority_view.as_ref());
                match followed_claim_target(file, item, (&disk_heads, &disk_view), authority) {
                    Some(resolved) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "{source}_resolved_followed_edit source={} item_bytes={} target_bytes={} (#claimfollowedit)",
                                resolved.1.as_str(),
                                item.trim().len(),
                                resolved.0.trim().len()
                            ),
                        );
                        resolved
                    }
                    None => return Err(miss.into()),
                }
            }
        }
        Err(miss) => return Err(miss.into()),
    };
    if view == ClaimHeadSource::EditorAuthority
        && agent_doc_queue::queue_claim::resolve_claim_target(item, &disk_heads).is_err()
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_resolved source={} reason=head_not_yet_saved_to_disk item_bytes={}",
                view.as_str(),
                item.trim().len()
            ),
        );
    }
    let queues = match view {
        ClaimHeadSource::Disk => disk_view,
        ClaimHeadSource::EditorAuthority => authority_view.unwrap_or_default(),
    };
    Ok((target, queues))
}

/// The live head a claim that followed an operator edit away from `item`
/// now holds (`#claimfollowedit`), in the authoritative view. The editor cut
/// is authoritative when present; disk is used only while detached. `None`
/// when no single claim followed away from `item`. Rows may carry their
/// lifecycle projection (`#claimrowmigrate`).
fn followed_claim_target(
    file: &Path,
    item: &str,
    disk: (&[String], &ClaimQueueView),
    authority: Option<(&[String], &ClaimQueueView)>,
) -> Option<(String, ClaimHeadSource)> {
    let stored = load_ledger(file).ok()?;
    if stored.claims.is_empty() {
        return None;
    }
    let ((rows, queues), view) = authority
        .map(|authority| (authority, ClaimHeadSource::EditorAuthority))
        .unwrap_or((disk, ClaimHeadSource::Disk));
    let mut ledger = stored;
    ledger.follow_edits_in_view(now_secs(), queues);
    let heads = row_head_texts(rows);
    if let Some(text) = ledger.followed_item_text(item)
        && let Ok(target) = agent_doc_queue::queue_claim::resolve_claim_target(text, &heads)
    {
        return Some((target, view));
    }
    None
}

/// Parents of a short leaf that names more than one live row, in queue order.
///
/// `None` when the item is an exact head or names fewer than two nest vectors.
/// The authoritative text is the editor cut when one exists, otherwise disk.
fn shared_leaf_claim_candidates(
    file: &Path,
    item: &str,
) -> Result<Option<(Vec<String>, ClaimQueueView)>> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("read {} to resolve queue item {item:?}", file.display()))?;
    let authority = editor_authority_text(file, "queue_claim");
    let text = authority.as_deref().unwrap_or(content.as_str());
    let heads = all_queue_head_texts(text).unwrap_or_default();
    let matches = nest_leaf_matches(item, &heads);
    if matches.len() < 2 {
        return Ok(None);
    }
    Ok(Some((
        matches,
        ClaimQueueView::from_content(text).unwrap_or_default(),
    )))
}

struct PreparedSharedLeaf {
    target: String,
    identity: agent_doc_element_queue::QueueItemIdentity,
    claim_time_row: Option<String>,
    tracked_item_aliases: Vec<String>,
    dispatch_policy: Option<agent_doc_queue::queue_claim::SubagentDispatchPolicy>,
    lineage_generation_floor: u64,
}

/// Bind the first unclaimed parent of a shared leaf. A later claim of the same
/// leaf takes the next parent. Every row stays in the document.
fn claim_next_shared_leaf(
    file: &Path,
    candidates: Vec<String>,
    queues: ClaimQueueView,
    owner: &str,
    ttl_secs: u64,
    owner_session: Option<ClaimOwnerSession>,
) -> Result<(ClaimOutcome, QueueClaim)> {
    let mut prepared = Vec::with_capacity(candidates.len());
    for target in &candidates {
        let identity = claim_identity(target);
        let authorization =
            crate::subagent_dispatch::subagent_authorization_for_item(file, target)?;
        let tracked_item_aliases = if identity.is_id_backed() {
            authorization
                .tracked_items
                .iter()
                .map(|item| item.text.trim().to_string())
                .filter(|text| !text.is_empty())
                .collect()
        } else {
            Vec::new()
        };
        let dispatch_policy = (authorization.model.is_some() || authorization.effort.is_some())
            .then_some(agent_doc_queue::queue_claim::SubagentDispatchPolicy {
                model: authorization.model,
                effort: authorization.effort,
            });
        prepared.push(PreparedSharedLeaf {
            target: target.clone(),
            identity,
            claim_time_row: nested_claim_row_at_claim(file, target),
            tracked_item_aliases,
            dispatch_policy,
            lineage_generation_floor: crate::queue_lineage::max_generation(file, target)?,
        });
    }
    let claim_keys = queues.claim_keys();
    let now = now_secs();
    let owner_session_for_claim = owner_session.clone();
    let result = mutate_ledger(file, |ledger| {
        let chosen = prepared
            .iter()
            .find(|item| {
                !ledger
                    .claims
                    .iter()
                    .any(|claim| claim.identity == item.identity && claim.blocks_dispatch(now))
            })
            .unwrap_or(&prepared[0]);
        let outcome = ledger.claim_in_session_with_generation_floor(
            &chosen.target,
            owner,
            now,
            ttl_secs,
            owner_session_for_claim.clone(),
            chosen.lineage_generation_floor,
        );
        log_followed(file, &ledger.follow_edits_in_view(now, &queues));
        let stored = ledger
            .claims
            .iter_mut()
            .find(|claim| claim.identity == chosen.identity)
            .context("claimed item missing from the ledger after claim")?;
        if matches!(outcome, ClaimOutcome::Created | ClaimOutcome::Reassigned)
            && stored.claim_time_row.is_none()
        {
            stored.claim_time_row = chosen.claim_time_row.clone();
        }
        stored.dispatch_policy = chosen.dispatch_policy.clone();
        stored.tracked_item_aliases = chosen.tracked_item_aliases.clone();
        Ok((outcome, stored.clone()))
    })?;
    let occurrences = claim_keys
        .iter()
        .filter(|head| claim_identity(head) == result.1.identity)
        .count();
    persist_claim_result(file, result, occurrences)
}

fn persist_claim_result(
    file: &Path,
    result: (ClaimOutcome, QueueClaim),
    occurrences: usize,
) -> Result<(ClaimOutcome, QueueClaim)> {
    if result.0 == ClaimOutcome::HeldByOther {
        bail!(
            "queue head {:?} is already held by {} generation {}; release/expire that attempt before redispatch",
            result.1.item_text,
            result.1.owner,
            result.1.generation
        );
    }
    crate::queue_lineage::record_claim(file, &result.1)
        .context("persist durable queue worktree lineage claim receipt")?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim outcome={:?} owner={} generation={} state={:?} expires_at={} item_bytes={} occurrences={occurrences} owner_session={} model={} effort={}",
            result.0,
            result.1.owner,
            result.1.generation,
            result.1.state,
            result.1.expires_at_secs,
            result.1.item_text.len(),
            owner_session_log_token(result.1.owner_session.as_ref()),
            result
                .1
                .dispatch_policy
                .as_ref()
                .and_then(|policy| policy.model.as_deref())
                .unwrap_or("-"),
            result
                .1
                .dispatch_policy
                .as_ref()
                .and_then(|policy| policy.effort.as_deref())
                .unwrap_or("-"),
        ),
    );
    Ok(result)
}

/// Claim the live queue head `item` for `owner` for `ttl_secs`.
///
/// Refuses an item that is not a live head of the document's queue: a claim on
/// nothing would silently do nothing, and a typo would leave the real head
/// drainable while the caller believes it is protected.
pub fn claim(
    file: &Path,
    item: &str,
    owner: &str,
    ttl_secs: u64,
) -> Result<(ClaimOutcome, QueueClaim)> {
    claim_with_session(file, item, owner, ttl_secs, current_owner_session())
}

/// [`claim`] bound to an explicit owner session (`#claimownerliveness`);
/// [`claim`] passes [`current_owner_session`].
pub fn claim_with_session(
    file: &Path,
    item: &str,
    owner: &str,
    ttl_secs: u64,
    owner_session: Option<ClaimOwnerSession>,
) -> Result<(ClaimOutcome, QueueClaim)> {
    if item.trim().is_empty() {
        bail!("queue claim needs a non-empty --item (`#id` or the queue line's text)");
    }
    if owner.trim().is_empty() {
        bail!("queue claim needs a non-empty --owner (e.g. `subagent:<label>`)");
    }
    // A short leaf under several parents is not a miss (`#nestlistlineage`).
    // Bind the first of those rows that no live claim holds. The others stay
    // live and are claimed by their own nest vector.
    if let Some((candidates, queues)) = shared_leaf_claim_candidates(file, item)? {
        return claim_next_shared_leaf(file, candidates, queues, owner, ttl_secs, owner_session);
    }
    // A miss is a typed usage error (`QueueClaimMiss`) naming the live heads,
    // never a bare string: the CLI reports it as-is instead of wrapping it in
    // the generic turn-failure notice.
    let (target, heads) = resolve_live_claim_target(file, item, "queue_claim")?;
    // The nest vector stored as `item_text` has no sub-prompt block. Keep the
    // expanded row, children included, so later siblings still split
    // (`#nestedclaimsteer`, `#nestlistlineage`).
    let claim_time_row = nested_claim_row_at_claim(file, &target);
    let identity = claim_identity(&target);
    let authorization = crate::subagent_dispatch::subagent_authorization_for_item(file, &target)?;
    let tracked_item_aliases: Vec<String> = if identity.is_id_backed() {
        authorization
            .tracked_items
            .iter()
            .map(|item| item.text.trim().to_string())
            .filter(|text| !text.is_empty())
            .collect()
    } else {
        Vec::new()
    };
    let dispatch_policy = (authorization.model.is_some() || authorization.effort.is_some())
        .then_some(agent_doc_queue::queue_claim::SubagentDispatchPolicy {
            model: authorization.model,
            effort: authorization.effort,
        });
    // `#queueeffectidentity`: preflight annotates intentional repeated effects,
    // so an exact identity normally names one occurrence. Keep the count in the
    // log as a rolling-upgrade diagnostic: a legacy unannotated claim still
    // anchors every identical copy until preflight projects effect identities.
    let occurrences = heads
        .claim_keys()
        .iter()
        .filter(|head| claim_identity(head) == identity)
        .count();
    // Live claims are pruned when their rows close, but lineage is retained.
    // Its generation floor prevents a later identical row from inheriting a
    // stale completion/integration receipt.
    let lineage_generation_floor = crate::queue_lineage::max_generation(file, &target)?;
    let now = now_secs();
    let result = mutate_ledger(file, |ledger| {
        let outcome = ledger.claim_in_session_with_generation_floor(
            &target,
            owner,
            now,
            ttl_secs,
            owner_session,
            lineage_generation_floor,
        );
        // Anchor the claim to its line(s) in the view it resolved in.
        log_followed(file, &ledger.follow_edits_in_view(now, &heads));
        let stored = ledger
            .claims
            .iter_mut()
            .find(|claim| claim.identity == identity)
            .context("claimed item missing from the ledger after claim")?;
        if matches!(outcome, ClaimOutcome::Created | ClaimOutcome::Reassigned)
            && stored.claim_time_row.is_none()
            && let Some(row) = claim_time_row
        {
            stored.claim_time_row = Some(row);
        }
        stored.dispatch_policy = dispatch_policy.clone();
        stored.tracked_item_aliases = tracked_item_aliases.clone();
        Ok((outcome, stored.clone()))
    })?;
    persist_claim_result(file, result, occurrences)
}

/// Positively acknowledge that the harness successfully dispatched one
/// concrete worker for one exact claim generation (`#queueclaimworker`).
/// This is the only transition from `Claimed`/🟡 to `Running`/🚧.
pub fn start_attempt(
    file: &Path,
    item: &str,
    owner: &str,
    worker_id: &str,
    ttl_secs: u64,
    generation: u64,
) -> Result<QueueClaim> {
    if item.trim().is_empty() {
        bail!("queue start needs a non-empty --item (`#id` or the queue line's text)");
    }
    if owner.trim().is_empty() {
        bail!("queue start needs a non-empty --owner (the claim's holder)");
    }
    if worker_id.trim().is_empty() {
        bail!("queue start needs a non-empty --worker-id from the successful harness dispatch");
    }
    let (target, heads) = resolve_live_claim_target(file, item, "queue_claim_start")?;
    let now = now_secs();
    let owner_session = current_owner_session();
    let outcome = mutate_ledger(file, |ledger| {
        // Resolution follows edits on a read-only ledger copy. Persist that
        // same authoritative rekey in the transaction before addressing the
        // attempt by its current text; otherwise `claims` can show the edited
        // item while `start` still reports the attempt missing.
        log_followed(file, &ledger.follow_edits_in_view(now, &heads));
        let mut claim = ledger
            .start(&target, owner, worker_id, now, ttl_secs, Some(generation))
            .map_err(anyhow::Error::from)?;
        if owner_session.is_some()
            && let Some(stored) = ledger
                .claims
                .iter_mut()
                .find(|stored| stored.identity == claim.identity)
        {
            stored.owner_session = owner_session.clone();
            claim.owner_session = owner_session.clone();
        }
        Ok(claim)
    });
    if let Ok(claim) = &outcome {
        crate::queue_lineage::record_started(file, claim)
            .context("persist durable queue worktree lineage start receipt")?;
    }
    match &outcome {
        Ok(claim) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_start outcome=running owner={} worker_id={} generation={} expires_at={} item_bytes={}",
                claim.owner,
                worker_id.trim(),
                claim.generation,
                claim.expires_at_secs,
                item.trim().len(),
            ),
        ),
        Err(error) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_start outcome=refused owner={} worker_id={} generation={} item_bytes={} error={error:#}",
                owner.trim(),
                worker_id.trim(),
                generation,
                item.trim().len(),
            ),
        ),
    }
    outcome
}

/// Extend `owner`'s live claim on `item` to `now + ttl_secs`
/// (`queue claim --refresh`). Refuses, with a typed
/// [`QueueClaimRefreshRefused`], when the item is unclaimed, has not been
/// started, already expired, or another owner holds it; it never creates or
/// promotes a claim.
pub fn refresh(file: &Path, item: &str, owner: &str, ttl_secs: u64) -> Result<QueueClaim> {
    refresh_at(file, item, owner, ttl_secs, now_secs())
}

pub fn refresh_attempt(
    file: &Path,
    item: &str,
    owner: &str,
    ttl_secs: u64,
    generation: Option<u64>,
) -> Result<QueueClaim> {
    refresh_attempt_at(file, item, owner, ttl_secs, now_secs(), generation)
}

fn refresh_at(file: &Path, item: &str, owner: &str, ttl_secs: u64, now: u64) -> Result<QueueClaim> {
    // Compatibility helper for internal callers/tests. Command boundaries use
    // `refresh_attempt` and enforce a generation fence.
    let generation = load_ledger(file)?
        .claims
        .into_iter()
        .find(|claim| claim.identity == claim_identity(item))
        .map(|claim| claim.generation);
    refresh_attempt_at(file, item, owner, ttl_secs, now, generation)
}

fn refresh_attempt_at(
    file: &Path,
    item: &str,
    owner: &str,
    ttl_secs: u64,
    now: u64,
    generation: Option<u64>,
) -> Result<QueueClaim> {
    if item.trim().is_empty() {
        bail!("queue claim --refresh needs a non-empty --item (`#id` or the queue line's text)");
    }
    if owner.trim().is_empty() {
        bail!("queue claim --refresh needs a non-empty --owner (the claim's holder)");
    }
    // Same resolution as `claim`, so `--item #id` refreshes a
    // `#subagents do [#id]` head and a closed head is a typed miss.
    let (target, heads) = resolve_live_claim_target(file, item, "queue_claim_refresh")?;
    // `#claimownerliveness`: a refresh also adopts the claim into the
    // caller's session, so a post-`/clear` coordinator that still hears from
    // the subagent makes the claim live again.
    let owner_session = current_owner_session();
    let outcome = mutate_ledger(file, |ledger| {
        log_followed(file, &ledger.follow_edits_in_view(now, &heads));
        Ok(ledger.refresh_attempt_in_session(
            &target,
            owner,
            now,
            ttl_secs,
            owner_session,
            generation,
        ))
    })?;
    match &outcome {
        Ok(claim) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_refresh outcome=refreshed owner={} generation={} state={:?} expires_at={} item_bytes={} owner_session={}",
                claim.owner,
                claim.generation,
                claim.state,
                claim.expires_at_secs,
                item.trim().len(),
                owner_session_log_token(claim.owner_session.as_ref()),
            ),
        ),
        Err(refused) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_refresh outcome=refused reason={} owner={} item_bytes={}",
                refused.reason.kind(),
                refused.owner,
                item.trim().len()
            ),
        ),
    }
    outcome.map_err(|refused: QueueClaimRefreshRefused| refused.into())
}

/// Mark one exact worker attempt as reported/awaiting integration.
pub fn report_attempt(file: &Path, item: &str, generation: Option<u64>) -> Result<QueueClaim> {
    report_attempt_with_publication_state(file, item, generation, None)
}

pub fn report_attempt_with_publication_state(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    publication_state: Option<agent_doc_element_queue::QueueReportState>,
) -> Result<QueueClaim> {
    let (target, heads) = resolve_live_claim_target(file, item, "queue_claim_report")?;
    let now = now_secs();
    let claim = mutate_ledger(file, |ledger| {
        log_followed(file, &ledger.follow_edits_in_view(now, &heads));
        ledger
            .report_with_publication_state(&target, generation, publication_state)
            .map_err(anyhow::Error::from)
    })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim_state state=reported owner={} generation={} item_bytes={}",
            claim.owner,
            claim.generation,
            claim.item_text.len()
        ),
    );
    Ok(claim)
}

/// First phase of a reported release. Persist `Reported` on the attempt before
/// its claim can be removed, so a crash between the claim and receipt writes
/// cannot expose the live row as bare. `None` preserves release's historical
/// no-op behavior when no claim exists; every other transition refusal remains
/// generation-fenced.
pub fn prepare_reported_release(file: &Path, item: &str) -> Result<Option<QueueClaim>> {
    prepare_reported_release_inner(file, item, None, false)
}

/// Generation-fenced [`prepare_reported_release`].
pub fn prepare_reported_release_attempt(
    file: &Path,
    item: &str,
    generation: Option<u64>,
) -> Result<Option<QueueClaim>> {
    prepare_reported_release_inner(file, item, generation, true)
}

fn prepare_reported_release_inner(
    file: &Path,
    item: &str,
    generation: Option<u64>,
    enforce_generation: bool,
) -> Result<Option<QueueClaim>> {
    let resolved = resolve_live_claim_target(file, item, "queue_claim_reported_release").ok();
    let target = resolved
        .as_ref()
        .map(|(target, _)| target.clone())
        .unwrap_or_else(|| item.to_string());
    let heads = resolved.map(|(_, heads)| heads);
    let now = now_secs();
    mutate_ledger(file, |ledger| {
        if let Some(heads) = heads.as_ref() {
            log_followed(file, &ledger.follow_edits_in_view(now, heads));
        }
        let report = |ledger: &mut QueueClaimLedger, candidate: &str| {
            let expected = if enforce_generation {
                generation
            } else {
                ledger
                    .claims
                    .iter()
                    .find(|claim| claim.identity == claim_identity(candidate))
                    .map(|claim| claim.generation)
            };
            ledger.report(candidate, expected)
        };
        match report(ledger, &target) {
            Ok(claim) => Ok(Some(claim)),
            Err(agent_doc_queue::queue_claim::QueueClaimTransitionRefused::Missing)
                if target != item =>
            {
                match report(ledger, item) {
                    Ok(claim) => Ok(Some(claim)),
                    Err(agent_doc_queue::queue_claim::QueueClaimTransitionRefused::Missing) => {
                        Ok(None)
                    }
                    Err(refused) => Err(refused.into()),
                }
            }
            Err(agent_doc_queue::queue_claim::QueueClaimTransitionRefused::Missing) => Ok(None),
            Err(refused) => Err(refused.into()),
        }
    })
}

/// Stable binary attestation key for one exact worker attempt. Generation and
/// the concrete `queue start` worker binding are part of every new key, so a
/// completion from an older or foreign worker cannot attest to a replacement
/// attempt even when queue text and owner label match.
pub fn completion_feedback_key(claim: &QueueClaim) -> Result<String> {
    let identity = serde_json::to_string(&claim.identity).context("serialize claim identity")?;
    let material = if claim.generation == 0 {
        format!("{identity}\n{}\n{}", claim.owner, claim.claimed_at_secs)
    } else {
        let worker_id = claim
            .worker
            .as_ref()
            .map(|worker| worker.worker_id.as_str())
            .context("generated claim completion requires a bound worker")?;
        return agent_doc_queue::queue_claim::completion_attestation_key(
            &claim.identity,
            &claim.owner,
            claim.generation,
            claim.claimed_at_secs,
            worker_id,
        )
        .context("serialize generated claim completion identity");
    };
    Ok(agent_doc_hash::content_hash(&material))
}

/// Recover a reported state from a durable binary completion cell for the
/// exact current attempt. This is deliberately bounded: feedback whose keyed
/// attempt no longer exists in the ledger is left quarantined and cannot
/// manufacture ownership for a newly claimed generation.
pub fn reconcile_completion_evidence(file: &Path, content: &str) -> Result<usize> {
    let recovered = mutate_ledger(file, |ledger| {
        let mut recovered = 0;
        for claim in &mut ledger.claims {
            if claim.state != agent_doc_queue::queue_claim::QueueClaimState::Running
                || claim.worker.is_none()
            {
                continue;
            }
            let current =
                agent_doc_document::subagent_completion::marker(&completion_feedback_key(claim)?);
            // Generation-zero claims compute the historical key above. New
            // claims never accept that unfenced form.
            if content.contains(&current) {
                claim.state = agent_doc_queue::queue_claim::QueueClaimState::Reported;
                recovered += 1;
            }
        }
        Ok(recovered)
    })?;
    if recovered > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("queue_claim_recovered_reported count={recovered}"),
        );
    }
    Ok(recovered)
}

/// Log-safe summary of a claim's owner session: harness, pane and pid, never
/// the session id itself.
fn owner_session_log_token(session: Option<&ClaimOwnerSession>) -> String {
    match session {
        None => "none".to_string(),
        Some(session) => format!(
            "{}@{}:{}",
            session.harness.as_deref().unwrap_or("-"),
            session.pane.as_deref().unwrap_or("-"),
            session
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    }
}

/// Release the claim on `item`. Returns the released claim, if there was one.
pub fn release(file: &Path, item: &str) -> Result<Option<QueueClaim>> {
    let resolved = resolve_live_claim_target(file, item, "queue_claim_release").ok();
    let target = resolved
        .as_ref()
        .map(|(target, _)| target.clone())
        .unwrap_or_else(|| item.to_string());
    let heads = resolved.map(|(_, heads)| heads);
    let now = now_secs();
    let released = mutate_ledger(file, |ledger| {
        if let Some(heads) = heads.as_ref() {
            log_followed(file, &ledger.follow_edits_in_view(now, heads));
        }
        Ok(ledger.release(&target).or_else(|| ledger.release(item)))
    })?;
    Ok(released)
}

pub fn release_attempt(
    file: &Path,
    item: &str,
    generation: Option<u64>,
) -> Result<Option<QueueClaim>> {
    // Release by the same resolution `claim` used, so `--item #id` releases a
    // `#subagents do [#id]` head; fall back to the literal item (a closed head
    // can still hold a stale claim worth releasing).
    let (target, heads) = match resolve_live_claim_target(file, item, "queue_claim_release") {
        Ok((target, heads)) => (target, Some(heads)),
        Err(err) => {
            if err.downcast_ref::<QueueClaimMiss>().is_none() {
                eprintln!(
                    "[queue-claim] WARNING: could not resolve {item:?} in {}; releasing it literally: {err:#}",
                    file.display()
                );
            }
            (item.to_string(), None)
        }
    };
    let now = now_secs();
    let released = mutate_ledger(file, |ledger| {
        if let Some(heads) = heads.as_ref() {
            log_followed(file, &ledger.follow_edits_in_view(now, heads));
        }
        match ledger.release_fenced(&target, generation)? {
            Some(claim) => Ok(Some(claim)),
            None => ledger
                .release_fenced(item, generation)
                .map_err(anyhow::Error::from),
        }
    })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim_release released={} item_bytes={}",
            released.is_some(),
            item.trim().len()
        ),
    );
    Ok(released)
}

/// End dispatch ownership without erasing the fact that this attempt started.
/// The retained `Lost` attempt is non-blocking and can be reclaimed as the next
/// generation, while its live queue row can never regress to bare.
pub fn abandon(file: &Path, item: &str) -> Result<Option<QueueClaim>> {
    let resolved = resolve_live_claim_target(file, item, "queue_claim_abandon").ok();
    let target = resolved
        .as_ref()
        .map(|(target, _)| target.clone())
        .unwrap_or_else(|| item.to_string());
    let heads = resolved.map(|(_, heads)| heads);
    let now = now_secs();
    mutate_ledger(file, |ledger| {
        if let Some(heads) = heads.as_ref() {
            log_followed(file, &ledger.follow_edits_in_view(now, heads));
        }
        let released = ledger.release(&target).or_else(|| ledger.release(item));
        Ok(released.map(|mut claim| {
            claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
            ledger.claims.push(claim.clone());
            claim
        }))
    })
}

/// Generation-fenced [`abandon`].
pub fn abandon_attempt(
    file: &Path,
    item: &str,
    generation: Option<u64>,
) -> Result<Option<QueueClaim>> {
    let resolved = resolve_live_claim_target(file, item, "queue_claim_abandon").ok();
    let target = resolved
        .as_ref()
        .map(|(target, _)| target.clone())
        .unwrap_or_else(|| item.to_string());
    let heads = resolved.map(|(_, heads)| heads);
    let now = now_secs();
    mutate_ledger(file, |ledger| {
        if let Some(heads) = heads.as_ref() {
            log_followed(file, &ledger.follow_edits_in_view(now, heads));
        }
        let released = match ledger.release_fenced(&target, generation)? {
            Some(claim) => Some(claim),
            None => ledger.release_fenced(item, generation)?,
        };
        Ok(released.map(|mut claim| {
            claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
            ledger.claims.push(claim.clone());
            claim
        }))
    })
}

/// Move one exact attempt into the durable operator-cancellation audit ledger.
/// The generation fence makes delayed cancellation harmless after a same-text
/// re-add has already created a replacement attempt.
pub fn cancel_removed_attempt(
    file: &Path,
    item: &str,
    generation: Option<u64>,
) -> Result<QueueClaimCancellationReceipt> {
    let cancelled_at_secs = now_secs();
    let receipt = mutate_ledger(file, |ledger| {
        ledger
            .cancel_attempt(item, generation, cancelled_at_secs)
            .map_err(anyhow::Error::from)
    })?;
    crate::queue_lineage::record_cancelled_claim(
        file,
        &receipt.claim,
        receipt.cancelled_at_secs,
        Some("operator removed active row".to_string()),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim_cancelled owner={} worker={} generation={} item_bytes={} completion_key={} reason=operator_removed (#queue-removal-cancels)",
            receipt.claim.owner,
            receipt
                .claim
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str())
                .unwrap_or("-"),
            receipt.claim.generation,
            receipt.claim.item_text.trim().len(),
            receipt.completion_key.as_deref().unwrap_or("-"),
        ),
    );
    Ok(receipt)
}

/// Preserve a cancellation tombstone when the worker's reported claim was
/// already converted into a durable release receipt.
pub fn record_released_cancellation(
    file: &Path,
    reported: &agent_doc_queue::claim_release_receipt::ClaimReleaseReceipt,
) -> Result<QueueClaimCancellationReceipt> {
    let worker = reported.worker_id.as_ref().map(|worker_id| {
        agent_doc_queue::queue_claim::QueueWorkerBinding {
            worker_id: worker_id.clone(),
            started_at_secs: reported.claimed_at_secs,
        }
    });
    let claim = QueueClaim {
        identity: reported.identity.clone(),
        item_text: reported.item_text.clone(),
        owner: reported.owner.clone(),
        generation: reported.generation,
        state: agent_doc_queue::queue_claim::QueueClaimState::Reported,
        publication_state: reported.publication_state,
        worker,
        dispatch_policy: None,
        tracked_item_aliases: Vec::new(),
        claimed_at_secs: reported.claimed_at_secs,
        expires_at_secs: reported.released_at_secs,
        owner_session: None,
        anchor: None,
        followed_from: Vec::new(),
        followed_generation: 0,
        claim_time_row: None,
    };
    let cancelled_at_secs = now_secs();
    let receipt = mutate_ledger(file, |ledger| {
        Ok(ledger.record_cancellation(claim, cancelled_at_secs, reported.completion_key.clone()))
    })?;
    crate::queue_lineage::record_cancelled_claim(
        file,
        &receipt.claim,
        receipt.cancelled_at_secs,
        Some("operator removed row after reported release".to_string()),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim_cancelled owner={} worker={} generation={} item_bytes={} completion_key={} reason=operator_removed_after_reported_release (#queue-removal-cancels)",
            receipt.claim.owner,
            receipt
                .claim
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str())
                .unwrap_or("-"),
            receipt.claim.generation,
            receipt.claim.item_text.trim().len(),
            receipt.completion_key.as_deref().unwrap_or("-"),
        ),
    );
    Ok(receipt)
}

/// Cross-process lock shared by `queue report-complete` and operator removal.
/// Whichever wins publishes a monotonic successor; report replay can never
/// slip a completion cell behind the cancellation tombstone.
pub fn completion_feedback_lock_path(file: &Path) -> Result<PathBuf> {
    let root = agent_doc_fs::find_project_root(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    Ok(root
        .join(".agent-doc/locks")
        .join(format!("subagent-completion-{document_hash}.lock")))
}

/// Release the claim on a head the binary itself just closed (`#claimstrike`:
/// a finalize strike of an answered free-text head). Unlike [`release`] this
/// never resolves `head` against the live queue: the head is no longer live,
/// so resolution would only miss (and could cross the controller socket for
/// an editor-authority lookup inside closeout). The ledger matches by the
/// shared claim identity, which is marker-invariant.
pub fn release_closed_head(file: &Path, head: &str) -> Result<Option<QueueClaim>> {
    let released = mutate_ledger(file, |ledger| Ok(ledger.release(head)))?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim_release released={} item_bytes={} reason=closed_by_strike",
            released.is_some(),
            head.trim().len()
        ),
    );
    Ok(released)
}

/// Carry live claims from promoted free-text heads to their minted `do [#id]`
/// heads (`#freetextqueue`). `promotions` are exact `(source_head, id)`
/// receipts from free-text admission. A no-op (and no state.db access) when
/// nothing was promoted or the ledger holds no claims.
pub fn transfer_promoted_claims(
    file: &Path,
    promotions: &[(String, String)],
) -> Result<Vec<(String, String, String)>> {
    if promotions.is_empty() || load_ledger(file)?.claims.is_empty() {
        return Ok(Vec::new());
    }
    let now = now_secs();
    let transferred = mutate_ledger(file, |ledger| Ok(ledger.transfer_promoted(now, promotions)))?;
    for (source, id, owner) in &transferred {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_transferred_on_promotion owner={owner} id={id} source_bytes={} (#freetextqueue)",
                source.trim().len()
            ),
        );
    }
    Ok(transferred)
}

/// Active claims for `file` judged against `content`: expired claims and claims
/// on items no longer in the queue are excluded.
pub fn active_claims_for_content(file: &Path, content: &str) -> Result<Vec<QueueClaim>> {
    let ledger = load_ledger_following(file, content)?;
    if ledger.claims.is_empty() {
        return Ok(Vec::new());
    }
    let _guard = steer_guard(snapshots_from_ledger(&ledger));
    let live = live_queue_head_identities(content);
    Ok(ledger
        .active(now_secs(), live.as_ref())
        .into_iter()
        .cloned()
        .collect())
}

/// A claimed queue attempt whose harness spawn was never acknowledged with
/// `queue start`. This is an actionable coordinator obligation, not evidence
/// that a worker is running (`#queueclaimworker`, GH #301).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DispatchAcknowledgementObligation {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub start_command: String,
    pub release_command: String,
}

impl DispatchAcknowledgementObligation {
    pub fn message(&self) -> String {
        format!(
            "queue item {:?} is still claimed/dispatching by {} at generation {} but no worker \
             acknowledgement was recorded. If the harness spawn succeeded, bind its worker now: \
             `{}`. Otherwise abandon this attempt for redispatch: `{}`.",
            self.item, self.owner, self.generation, self.start_command, self.release_command,
        )
    }
}

fn shell_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn dispatch_acknowledgement_obligation(
    file: &Path,
    claim: &QueueClaim,
) -> DispatchAcknowledgementObligation {
    let item_handle = agent_doc_queue::queue_claim::claim_item_handle(&claim.item_text);
    let document = shell_single_quote(&file.display().to_string());
    let item = shell_single_quote(&item_handle);
    let owner = shell_single_quote(&claim.owner);
    DispatchAcknowledgementObligation {
        item: claim.item_text.clone(),
        owner: claim.owner.clone(),
        generation: claim.generation,
        start_command: format!(
            "agent-doc queue start {document} --item {item} --owner {owner} --generation {} --worker-id <worker-id>",
            claim.generation,
        ),
        release_command: format!(
            "agent-doc queue release {document} --item {item} --generation {} --requeue",
            claim.generation,
        ),
    }
}

/// Derive every overdue dispatch acknowledgement from the current queue and
/// durable claim ledger. Callers supply time/grace so boundary tests stay
/// deterministic without sleeping.
pub fn dispatch_acknowledgement_obligations_for_content_at(
    file: &Path,
    content: &str,
    now_secs: u64,
    grace_secs: u64,
) -> Result<Vec<DispatchAcknowledgementObligation>> {
    Ok(active_claims_for_content(file, content)?
        .into_iter()
        .filter(|claim| {
            agent_doc_queue::queue_claim::dispatch_evidence_missing(claim, now_secs, grace_secs)
        })
        .map(|claim| dispatch_acknowledgement_obligation(file, &claim))
        .collect())
}

/// Production projection of overdue dispatch acknowledgements.
pub fn dispatch_acknowledgement_obligations_for_content(
    file: &Path,
    content: &str,
) -> Result<Vec<DispatchAcknowledgementObligation>> {
    dispatch_acknowledgement_obligations_for_content_at(
        file,
        content,
        now_secs(),
        agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS,
    )
}

/// Every live queue-head claim record, including lost and reported states, for
/// the visible runtime projection. Expired active attempts are presented as
/// lost even before the next durable prune pass.
pub fn claim_states_for_content(file: &Path, content: &str) -> Result<Vec<QueueClaim>> {
    let ledger = load_ledger_following(file, content)?;
    let live = live_queue_head_identities(content);
    let now = now_secs();
    Ok(ledger
        .claims
        .into_iter()
        .filter(|claim| {
            live.as_ref()
                .is_some_and(|live| live.contains(&claim.identity))
        })
        .map(|mut claim| {
            if claim.state != agent_doc_queue::queue_claim::QueueClaimState::Reported
                && claim.is_expired(now)
            {
                claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
            }
            claim
        })
        .collect())
}

/// Actionable recovery for a lost/expired claim shown by `queue claims`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LostClaimRecovery {
    /// Why the attempt no longer owns the queue head.
    pub reason: &'static str,
    /// What to do when the bound worker may have completed after expiry.
    pub recover_completed_worker_result: String,
    /// Exact command that creates the next fenced generation when no usable
    /// worker result exists.
    pub reclaim_command: String,
}

/// One live queue-head claim attempt with its owner-session verdict, for
/// `queue claims` (`#claimownerliveness`). This includes retained `Lost`
/// attempts: hiding an expired attempt leaves the visible `⚠️` marker without
/// either its cause or an actionable recovery.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClaimReport {
    #[serde(flatten)]
    pub claim: QueueClaim,
    /// `live`, `unrecorded`, or `ended`.
    pub owner_session_state: &'static str,
    /// Why the owner session ended, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_session_end_reason: Option<&'static str>,
    /// Binary assessment of the durable attempt evidence. This is deliberately
    /// narrower than "the worker is making progress": agent-doc can prove a
    /// worker binding and coordinator-session liveness, but cannot observe the
    /// worker's current activity or an external job it is waiting on.
    pub assessment: &'static str,
    /// Operator-facing interpretation of `assessment`.
    pub assessment_detail: String,
    /// Age of the attempt from its original claim boundary.
    pub claim_age_secs: u64,
    /// Time until the attempt expires. Zero for an expired/lost attempt.
    pub ttl_remaining_secs: u64,
    /// Age of the positive worker-dispatch acknowledgement, when one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_started_age_secs: Option<u64>,
    /// Present for a retained lost attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lost_recovery: Option<LostClaimRecovery>,
    /// Whether operator-visible queue markers agree with durable claim state.
    pub visible_projection_state: &'static str,
    pub expected_visible_marker: String,
    pub observed_visible_markers: Vec<String>,
    /// Exact argv for state-preserving repair when projection is out of sync.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action_argv: Option<Vec<String>>,
}

fn claim_assessment(claim: &QueueClaim, owner_session_state: &str) -> (&'static str, String) {
    use agent_doc_queue::queue_claim::QueueClaimState;

    match claim.state {
        QueueClaimState::Lost => (
            "lost",
            "The attempt expired or was abandoned; it is recovery-eligible and does not prove current work.".to_string(),
        ),
        QueueClaimState::Reported => (
            "reported_pending_integration",
            "The worker reported completion; coordinator integration, verification, and queue closeout are still pending.".to_string(),
        ),
        QueueClaimState::Claimed => (
            if owner_session_state == "ended" {
                "stale_owner_session_ended"
            } else {
                "dispatching_no_worker"
            },
            if owner_session_state == "ended" {
                "The claiming coordinator session ended and no worker is bound; nothing proves this attempt is running.".to_string()
            } else {
                "The head is reserved for dispatch, but no worker is bound; this is not evidence that a worker is running.".to_string()
            },
        ),
        QueueClaimState::Running if owner_session_state == "ended" => (
            "stale_owner_session_ended",
            "A worker was bound, but the claiming coordinator session ended; current progress is unverifiable until the claim is adopted or released.".to_string(),
        ),
        QueueClaimState::Running if owner_session_state == "unrecorded" => (
            "worker_bound_owner_unrecorded",
            "A worker dispatch is acknowledged, but coordinator-session liveness was not recorded. Agent-doc cannot observe worker progress or external jobs; use the expiry and inspect the worker before deciding whether the claim is stale.".to_string(),
        ),
        QueueClaimState::Running => (
            "worker_bound_owner_live",
            "A worker dispatch is acknowledged and the coordinator session is live. Agent-doc cannot observe worker progress or external jobs; an old claim timestamp alone does not make the claim stale.".to_string(),
        ),
    }
}

fn lost_claim_recovery(file: &Path, claim: &QueueClaim, now: u64) -> LostClaimRecovery {
    let document = shell_single_quote(&file.display().to_string());
    let item = shell_single_quote(&agent_doc_queue::queue_claim::claim_item_handle(
        &claim.item_text,
    ));
    let owner = shell_single_quote(&claim.owner);
    let worker = claim
        .worker
        .as_ref()
        .map(|binding| binding.worker_id.as_str())
        .unwrap_or("the prior worker");
    LostClaimRecovery {
        reason: if claim.is_expired(now) {
            "claim_ttl_expired"
        } else {
            "attempt_abandoned_for_requeue"
        },
        recover_completed_worker_result: format!(
            "Check {worker} for a completed result. If it completed, integrate that exact result and close the queue head through the normal agent-doc response (`--done <id>` for an id-backed head or the exact `> **Queue prompt:**` echo for free text)."
        ),
        reclaim_command: format!("agent-doc queue claim {document} --item {item} --owner {owner}"),
    }
}

/// Every live queue-head attempt, including retained lost attempts, with its
/// owner-session verdict and actionable expiry recovery.
pub fn claim_reports_for_content(file: &Path, content: &str) -> Result<Vec<ClaimReport>> {
    let components = agent_doc_element::element::parse(content)?;
    let observed = components
        .iter()
        .filter(|component| component.name == "queue")
        .flat_map(|component| {
            let entries = agent_doc_queue::document_queue::parse(component.content(content))
                .unwrap_or_default();
            entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| match entry {
                    agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                    | agent_doc_queue::document_queue::QueueEntry::Completed(prompt) => {
                        let identity =
                            agent_doc_queue::queue_claim::claim_identity_at(&entries, index)
                                .unwrap_or_else(|| {
                                    agent_doc_queue::queue_claim::claim_identity(&prompt.text)
                                });
                        Some((
                            identity,
                            agent_doc_document::queue_projection::lifecycle_marker(&prompt.text)
                                .map(|marker| marker.prefix().to_string())
                                .unwrap_or_else(|| "bare".to_string()),
                        ))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let now = claim_clock_now_secs();
    Ok(claim_states_for_content(file, content)?
        .into_iter()
        .map(|claim| {
            let (state, reason) = match claim_owner_liveness(file, &claim) {
                ClaimOwnerLiveness::Live => ("live", None),
                ClaimOwnerLiveness::Unrecorded => ("unrecorded", None),
                ClaimOwnerLiveness::Ended(reason) => ("ended", Some(reason.as_str())),
            };
            let expected_visible_marker = claim.state.projection_marker().prefix().to_string();
            let observed_visible_markers = observed
                .iter()
                .filter(|(identity, _)| identity == &claim.identity)
                .map(|(_, marker)| marker.clone())
                .collect::<Vec<_>>();
            let synchronized = !observed_visible_markers.is_empty()
                && observed_visible_markers
                    .iter()
                    .all(|marker| marker == &expected_visible_marker);
            let (assessment, assessment_detail) = claim_assessment(&claim, state);
            ClaimReport {
                assessment,
                assessment_detail,
                claim_age_secs: now.saturating_sub(claim.claimed_at_secs),
                ttl_remaining_secs: claim.expires_at_secs.saturating_sub(now),
                worker_started_age_secs: claim
                    .worker
                    .as_ref()
                    .map(|worker| now.saturating_sub(worker.started_at_secs)),
                lost_recovery: (claim.state == agent_doc_queue::queue_claim::QueueClaimState::Lost)
                    .then(|| lost_claim_recovery(file, &claim, now)),
                claim,
                owner_session_state: state,
                owner_session_end_reason: reason,
                visible_projection_state: if synchronized {
                    "synchronized"
                } else {
                    "out_of_sync"
                },
                expected_visible_marker,
                observed_visible_markers,
                next_action_argv: (!synchronized).then(|| {
                    vec![
                        "agent-doc".to_string(),
                        "queue".to_string(),
                        "repair-projection".to_string(),
                        file.to_string_lossy().into_owned(),
                    ]
                }),
            }
        })
        .collect())
}

/// The claimed heads the in-session drainability filter must skip.
///
/// A ledger that cannot be read is reported and treated as "no claims": the
/// failure direction is the pre-claim behaviour (the head stays drainable),
/// never a stranded queue.
pub fn claimed_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    let _guard = steer_guard(nested_claim_steer_snapshots(file, content));
    claimed_items_with_non_ledger_holds(
        file,
        content,
        ledger_claimed_items_for_content(file, content),
    )
}

/// The claimed heads an in-session writeback computation must skip.
///
/// A `Reported` claim still fences redispatch, but its worker result is now the
/// coordinator's writeback debt. Therefore only `Claimed` and `Running`
/// attempts are excluded here. Durable completion evidence and queue-level
/// holds remain excluded exactly as they are in [`claimed_items_for_content`].
/// An unreadable ledger contributes no attempt claims (the queue head remains
/// visible instead of becoming stranded).
pub fn in_flight_claimed_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    use agent_doc_queue::queue_claim::QueueClaimState;

    let _guard = steer_guard(nested_claim_steer_snapshots(file, content));
    let in_flight = match active_claims_for_content(file, content) {
        Ok(claims) => ClaimedQueueItems::from_identities(
            claims
                .into_iter()
                .filter(|claim| {
                    matches!(
                        claim.state,
                        QueueClaimState::Claimed | QueueClaimState::Running
                    )
                })
                .map(|claim| claim.identity),
        ),
        Err(err) => {
            eprintln!(
                "[queue-claim] WARNING: could not read queue claims for {}; treating every head as unclaimed: {err:#}",
                file.display()
            );
            ClaimedQueueItems::none()
        }
    };
    claimed_items_with_queue_holds(file, content, in_flight)
}

fn claimed_items_with_non_ledger_holds(
    file: &Path,
    content: &str,
    claimed: ClaimedQueueItems,
) -> ClaimedQueueItems {
    let claimed = claimed.with_heads(&completion_evidence_hold_texts(content));
    claimed_items_with_queue_holds(file, content, claimed)
}

/// Add structural queue holds that remain out of the in-session loop regardless
/// of worker lifecycle. Completion evidence is intentionally not added here:
/// it proves a worker reported and therefore creates coordinator writeback debt,
/// not in-flight worker ownership (`#reportedwritebackdebt`).
fn claimed_items_with_queue_holds(
    file: &Path,
    content: &str,
    claimed: ClaimedQueueItems,
) -> ClaimedQueueItems {
    // The queue-level subagents attribute holds heads back (cap full, or an
    // `after=` predecessor still queued). A held head is out of the in-session
    // loop exactly like a claimed one: never drained inline, never counted.
    let claimed = match crate::subagent_dispatch::queue_attr_subagent_plan(file, content) {
        Some(plan) if !plan.held.is_empty() => claimed.with_heads(&plan.held),
        _ => claimed,
    };
    // `#queueschedule`: a scheduled queue's closed window holds every head the
    // same way — out of the in-session loop, the supervisor drain, preflight
    // selection, and the Stop-hook continuation until a tick opens a pass.
    let schedule_held = crate::queue_schedule::schedule_held_heads(file, content);
    if schedule_held.is_empty() {
        claimed
    } else {
        claimed.with_heads(&schedule_held)
    }
}

/// `#capturegatedispatched`: the live heads of `content` a worker owns — an
/// unexpired claim whose worker started (`Running`) or reported (`Reported`,
/// awaiting integration), or a head with a binary-attested completion cell.
/// A dispatch claim no worker acknowledged (`Claimed`), a lost or expired
/// attempt, and a subagents-attribute hold are not worker ownership. An
/// unreadable ledger contributes no claims (the gate this feeds stays armed).
pub fn worker_owned_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    use agent_doc_queue::queue_claim::QueueClaimState;
    let claims = claim_states_for_content(file, content).unwrap_or_else(|err| {
        eprintln!(
            "[queue-claim] WARNING: could not read queue claims for {}; treating every head as unowned: {err:#}",
            file.display()
        );
        Vec::new()
    });
    ClaimedQueueItems::from_identities(
        claims
            .into_iter()
            .filter(|claim| {
                matches!(
                    claim.state,
                    QueueClaimState::Running | QueueClaimState::Reported
                )
            })
            .map(|claim| claim.identity),
    )
    .with_heads(&completion_evidence_hold_texts(content))
}

/// Rows whose own work has a structurally binary-attested completion cell.
///
/// The first string is the marker-stripped row (what a completion heading
/// quotes). The second is the claim key: the nest vector for a nested
/// non-tracked leaf, otherwise the same text (`#nestlistlineage`). Matching
/// the vector alone dropped `unblock the sample` under `do [#alpha]` because
/// the cell quotes the child, not `#alpha: unblock the sample`.
fn completion_evidence_rows(content: &str) -> Vec<(String, String)> {
    let items = agent_doc_document::subagent_completion::completion_evidence(content)
        .into_iter()
        .map(|evidence| evidence.item)
        .collect::<Vec<_>>();
    if items.is_empty() {
        return Vec::new();
    }
    agent_doc_queue::queue_continuation::live_executable_row_keys(content)
        .unwrap_or_default()
        .into_iter()
        .filter(|(display, lineage)| {
            items.iter().any(|item| {
                agent_doc_queue::queue_claim::completion_evidence_matches_row(item, display)
                    || (lineage != display
                        && agent_doc_queue::queue_claim::completion_evidence_matches_row(
                            item, lineage,
                        ))
            })
        })
        .collect()
}

/// Live queue rows for which the document contains a structurally
/// binary-attested subagent completion cell. This is durable work evidence even
/// when an older claim was pruned and its generation can no longer be proven.
/// Such a row is held out of fresh dispatch/drain and projected as reconciling.
///
/// Each string is the row's own marker-stripped work, not its nest vector, so
/// a child cell still names the child.
pub fn completion_evidence_head_texts(content: &str) -> Vec<String> {
    completion_evidence_rows(content)
        .into_iter()
        .map(|(display, _)| display)
        .collect()
}

/// Claim keys for [`completion_evidence_head_texts`].
///
/// A nested non-tracked leaf is held as its nest vector, so the hold stays on
/// that row and does not cover the same leaf under another parent.
pub(crate) fn completion_evidence_hold_texts(content: &str) -> Vec<String> {
    completion_evidence_rows(content)
        .into_iter()
        .map(|(_, lineage)| lineage)
        .collect()
}

/// Live queue-head texts of `content` held by an active worker claim
/// (`#deferstrike`). A claimed head is owned by its worker, not by the cycle
/// that happens to see it: the selected-free-text evidence gate must not demand
/// an answer for it, and `#ftstrike` must not strike it. An unreadable ledger
/// is reported and treated as "no claims" (the pre-claim behaviour).
pub fn claimed_live_head_texts_for_content(file: &Path, content: &str) -> Vec<String> {
    match load_ledger(file) {
        Ok(ledger) => ledger.claimed_live_head_texts(now_secs(), content),
        Err(err) => {
            eprintln!(
                "[queue-claim] WARNING: could not read queue claims for {}; treating every head as unclaimed: {err:#}",
                file.display()
            );
            Vec::new()
        }
    }
}

/// Live queue-head texts that still match a durable claim attempt, including
/// attempts whose ownership TTL has expired. This is completion authority, not
/// dispatch ownership: closeout uses it only together with an exact,
/// materialized queue-prompt echo so a long-running coordinator can retire the
/// work its worker completed without keeping an expired claim live.
pub fn claim_attempt_head_texts_for_content(file: &Path, content: &str) -> Vec<String> {
    match load_ledger_following(file, content) {
        Ok(ledger) if ledger.claims.is_empty() => Vec::new(),
        Ok(ledger) => {
            let _guard = steer_guard(snapshots_from_ledger(&ledger));
            let attempts = ClaimedQueueItems::from_identities(
                ledger.claims.iter().map(|claim| claim.identity.clone()),
            );
            live_queue_head_texts(content)
                .unwrap_or_default()
                .into_iter()
                .filter(|head| attempts.claims(head))
                .collect()
        }
        Err(err) => {
            eprintln!(
                "[queue-claim] WARNING: could not read queue claim attempts for {}; treating every head as without attempt authority: {err:#}",
                file.display()
            );
            Vec::new()
        }
    }
}

/// Only the worker claims recorded in the ledger, without heads the queue
/// subagents attribute holds back.
pub fn ledger_claimed_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    match load_ledger_following(file, content) {
        Ok(ledger) if ledger.claims.is_empty() => ClaimedQueueItems::none(),
        Ok(ledger) => {
            let _guard = steer_guard(snapshots_from_ledger(&ledger));
            let live = live_queue_head_identities(content);
            ledger.claimed_items(now_secs(), live.as_ref())
        }
        Err(err) => {
            eprintln!(
                "[queue-claim] WARNING: could not read queue claims for {}; treating every head as unclaimed: {err:#}",
                file.display()
            );
            ClaimedQueueItems::none()
        }
    }
}

/// Durably drop expired claims and claims whose item has closed. Called at
/// closeout reconciliation, where a consumed head ends its claim.
pub fn prune_closed_claims(file: &Path, content: &str) -> Result<usize> {
    if load_ledger(file)?.claims.is_empty() {
        return Ok(0);
    }
    let _guard = steer_guard(nested_claim_steer_snapshots(file, content));
    let live = live_queue_head_identities(content);
    let heads = ClaimQueueView::from_content(content);
    let now = now_secs();
    let pruned = mutate_ledger(file, |ledger| {
        // Follow against the closeout's content (the disk read inside
        // `mutate_ledger` may lag it) before closure is judged.
        if let Some(heads) = &heads {
            log_followed(file, &ledger.follow_edits_in_view(now, heads));
        }
        Ok(ledger.prune_detailed(now, live.as_ref()))
    })?;
    // `#claimfollowedit`: name every closure and every active->lost transition,
    // so expiry is visible without pretending the attempt record disappeared.
    for (claim, cause) in &pruned {
        let event = match cause {
            agent_doc_queue::queue_claim::ClaimPruneCause::Expired => {
                "queue_claim_state state=lost"
            }
            agent_doc_queue::queue_claim::ClaimPruneCause::Closed => "queue_claim_pruned",
        };
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{event} owner={} generation={} cause={} item_bytes={} followed_edits={}",
                claim.owner,
                claim.generation,
                cause.as_str(),
                claim.item_text.trim().len(),
                claim.followed_from.len()
            ),
        );
    }
    if !pruned.is_empty() {
        agent_doc_ops_log_io::log_op(file, &format!("queue_claim_prune pruned={}", pruned.len()));
    }
    Ok(pruned.len())
}

/// Durably end the finished attempts of the rows a strike closed
/// (`#strikeretires`), at the strike's own mutation boundary rather than at
/// the next closeout prune. `struck_heads` are the rows the strike successor
/// `after` struck ([`heads_struck_by_successor`]); each is keyed by its work
/// identity, the row text after its attempt projection prefix
/// ([`strip_attempt_projection_prefix`]). A `Reported` or `Lost` attempt
/// ends; a dispatching or running attempt, or one whose identity still has a
/// live row in `after` (a mirror copy, a duplicate), stays. Lineage keeps the
/// generation floor, so a re-added identical row still claims a fresh
/// generation. Call this only after the strike successor was accepted, so a
/// failed write never ends the claim of a still-live row.
///
/// [`heads_struck_by_successor`]: agent_doc_queue::queue_consume::heads_struck_by_successor
/// [`strip_attempt_projection_prefix`]: agent_doc_element_queue::strip_attempt_projection_prefix
pub fn retire_struck_claims(
    file: &Path,
    struck_heads: &[String],
    after: &str,
    source: &str,
) -> Result<Vec<QueueClaim>> {
    if struck_heads.is_empty() || load_ledger(file)?.claims.is_empty() {
        return Ok(Vec::new());
    }
    let struck = struck_heads
        .iter()
        .map(|head| {
            claim_identity(agent_doc_element_queue::strip_attempt_projection_prefix(
                head,
            ))
        })
        .collect::<std::collections::HashSet<_>>();
    let live = agent_doc_queue::queue_consume::live_work_identities(after);
    let heads = ClaimQueueView::from_content(after);
    let now = now_secs();
    let retired = mutate_ledger(file, |ledger| {
        // Follow against the strike successor (the disk read inside
        // `mutate_ledger` may lag it) before the struck rows are judged.
        if let Some(heads) = &heads {
            log_followed(file, &ledger.follow_edits_in_view(now, heads));
        }
        Ok(ledger.retire_struck(now, &struck, &live))
    })?;
    for claim in &retired {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_retired_on_strike owner={} generation={} state={:?} source={source} item_bytes={} (#strikeretires)",
                claim.owner,
                claim.generation,
                claim.state,
                claim.item_text.trim().len(),
            ),
        );
    }
    Ok(retired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_continuation::live_queue_head_rows;

    fn write_doc(root: &Path, prompts: &[&str]) -> PathBuf {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        std::fs::write(
            &doc,
            format!(
                "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Queue\n\n<!-- agent:queue go -->\n{queue}<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        doc
    }

    /// `#finalgrace`: a pinned claim clock stamps the claim and measures its
    /// dispatch-acknowledgement age, so an unacknowledged claim stays inside
    /// the grace however long the test takes; advancing the pin past the grace
    /// raises the obligation, and dropping the guard restores the prior pin.
    #[test]
    fn pinned_claim_clock_makes_the_dispatch_grace_independent_of_run_speed() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#pinned]"]);
        let content = std::fs::read_to_string(&doc).unwrap();
        let pinned = 1_000_000;
        let _outer = pin_claim_clock_for_current_thread(pinned);
        let (_, claim) = claim(&doc, "#pinned", "subagent:pinned", 3600).unwrap();
        assert_eq!(claim.claimed_at_secs, pinned);
        assert!(
            dispatch_acknowledgement_obligations_for_content(&doc, &content)
                .unwrap()
                .is_empty(),
            "an unacknowledged claim inside the grace is not yet an obligation"
        );
        {
            let _later = pin_claim_clock_for_current_thread(
                pinned + agent_doc_queue::queue_claim::DISPATCH_EVIDENCE_GRACE_SECS,
            );
            assert_eq!(
                dispatch_acknowledgement_obligations_for_content(&doc, &content)
                    .unwrap()
                    .len(),
                1,
                "the same claim past the grace is a coordinator obligation"
            );
        }
        assert_eq!(
            claim_clock_now_secs(),
            pinned,
            "the inner pin restores the outer one"
        );
    }

    /// `#queueclaim`: a visible lost row remains diagnosable after its TTL.
    /// The read-only claims command must not hide the attempt or leave the
    /// operator with only an unexplained `⚠️` marker.
    #[test]
    fn claim_reports_include_expired_reason_and_actionable_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#lostdiag]"]);
        let content = std::fs::read_to_string(&doc).unwrap();
        let pinned = 1_000_000;
        let _outer = pin_claim_clock_for_current_thread(pinned);
        let (_, claim) = claim(&doc, "#lostdiag", "subagent:lostdiag", 10).unwrap();
        start_attempt(
            &doc,
            "#lostdiag",
            "subagent:lostdiag",
            "/root/lostdiag",
            10,
            claim.generation,
        )
        .unwrap();

        let _expired = pin_claim_clock_for_current_thread(pinned + 10);
        let reports = claim_reports_for_content(&doc, &content).unwrap();
        assert_eq!(reports.len(), 1, "{reports:#?}");
        let report = &reports[0];
        assert_eq!(
            report.claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Lost
        );
        assert_eq!(report.assessment, "lost");
        assert_eq!(report.claim_age_secs, 10);
        assert_eq!(report.ttl_remaining_secs, 0);
        assert_eq!(report.worker_started_age_secs, Some(10));
        let recovery = report.lost_recovery.as_ref().unwrap();
        assert_eq!(recovery.reason, "claim_ttl_expired");
        assert!(
            recovery
                .recover_completed_worker_result
                .contains("/root/lostdiag"),
            "{}",
            recovery.recover_completed_worker_result
        );
        assert!(
            recovery.reclaim_command.contains("--item '#lostdiag'")
                && recovery
                    .reclaim_command
                    .contains("--owner 'subagent:lostdiag'"),
            "{}",
            recovery.reclaim_command
        );
    }

    #[test]
    fn claim_reports_distinguish_expiry_from_explicit_requeue_abandonment() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#lostdiag]"]);
        let content = std::fs::read_to_string(&doc).unwrap();
        let pinned = 1_000_000;
        let _clock = pin_claim_clock_for_current_thread(pinned);
        let (_, claim) = claim(&doc, "#lostdiag", "subagent:lostdiag", 600).unwrap();
        start_attempt(
            &doc,
            "#lostdiag",
            "subagent:lostdiag",
            "/root/lostdiag",
            600,
            claim.generation,
        )
        .unwrap();
        abandon_attempt(&doc, "#lostdiag", Some(claim.generation)).unwrap();

        let reports = claim_reports_for_content(&doc, &content).unwrap();
        assert_eq!(reports.len(), 1, "{reports:#?}");
        assert_eq!(reports[0].assessment, "lost");
        assert_eq!(
            reports[0].lost_recovery.as_ref().unwrap().reason,
            "attempt_abandoned_for_requeue"
        );
    }

    /// `#claimfreshnessdiagnostics`: `Running` proves that dispatch bound a
    /// concrete worker, not that the worker or an external job is progressing.
    /// The claims projection exposes both the durable evidence and its limits.
    #[test]
    fn running_claim_report_distinguishes_worker_binding_from_current_progress() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#runningdiag]"]);
        let content = std::fs::read_to_string(&doc).unwrap();
        let pinned = 2_000_000;
        let _claimed_at = pin_claim_clock_for_current_thread(pinned);
        let (_, claim) = claim_with_session(
            &doc,
            "#runningdiag",
            "subagent:runningdiag",
            120,
            Some(ClaimOwnerSession {
                harness: Some("codex".to_string()),
                session_id: Some("runningdiag-session".to_string()),
                pane: None,
                pid: Some(std::process::id()),
            }),
        )
        .unwrap();
        start_attempt(
            &doc,
            "#runningdiag",
            "subagent:runningdiag",
            "/root/runningdiag",
            120,
            claim.generation,
        )
        .unwrap();

        let _observed_at = pin_claim_clock_for_current_thread(pinned + 40);
        let reports = claim_reports_for_content(&doc, &content).unwrap();
        assert_eq!(reports.len(), 1, "{reports:#?}");
        let report = &reports[0];
        assert_eq!(report.assessment, "worker_bound_owner_live");
        assert_eq!(report.claim_age_secs, 40);
        assert_eq!(report.ttl_remaining_secs, 80);
        assert_eq!(report.worker_started_age_secs, Some(40));
        assert!(
            report
                .assessment_detail
                .contains("cannot observe worker progress or external jobs"),
            "{}",
            report.assessment_detail
        );
        assert!(
            report
                .assessment_detail
                .contains("old claim timestamp alone does not make the claim stale"),
            "{}",
            report.assessment_detail
        );
    }

    #[test]
    fn claim_records_resolved_subagent_dispatch_policy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(
            &doc,
            concat!(
                "---\nsession: sid\nagent_doc_format: template\n",
                "subagent_model: default-model\nsubagent_effort: medium\n---\n\n",
                "## Queue\n\n<!-- agent:queue subagents model=queue-model effort=high go -->\n",
                "- fix policy\n<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        let (_, claimed) = claim(&doc, "fix policy", "subagent:policy", 3600).unwrap();
        let policy = claimed.dispatch_policy.as_ref().unwrap();
        assert_eq!(policy.model.as_deref(), Some("queue-model"));
        assert_eq!(policy.effort.as_deref(), Some("high"));
        let started = start_attempt(
            &doc,
            "fix policy",
            "subagent:policy",
            "worker:policy",
            3600,
            claimed.generation,
        )
        .unwrap();
        assert_eq!(started.dispatch_policy, claimed.dispatch_policy);
    }

    /// `#claimownerliveness`: the claiming session comes from the harness
    /// environment (a subagent inherits the coordinator's), and a claim made
    /// before the pane's `/clear` reads as ended once the pane's hook reports
    /// the new session — through the real `state.db` both sides use — while
    /// adoption by the new session makes it live again.
    #[test]
    fn claim_owner_session_is_bound_from_env_and_judged_against_the_pane() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.to_string())
            }
        };
        let claude = owner_session_from_env(env(&[
            ("CLAUDE_CODE_SESSION_ID", "pre-clear"),
            ("TMUX_PANE", "%claimliveness"),
            ("CLAUDE_PID", "4242"),
        ]))
        .unwrap();
        assert_eq!(claude.harness.as_deref(), Some("claude"));
        assert_eq!(claude.session_id.as_deref(), Some("pre-clear"));
        assert_eq!(claude.pid, Some(4242));
        let codex = owner_session_from_env(env(&[("CODEX_THREAD_ID", "t1")])).unwrap();
        assert_eq!(codex.harness.as_deref(), Some("codex"));
        assert_eq!(owner_session_from_env(env(&[])), None);

        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#cligenport]"]);
        let now = now_secs();
        mutate_ledger(&doc, |ledger| {
            ledger.claim_in_session(
                "do [#cligenport]",
                "subagent:cligoshell-then-cligenport",
                now,
                7200,
                Some(claude.clone()),
            );
            Ok(())
        })
        .unwrap();
        let claim = load_ledger(&doc).unwrap().claims[0].clone();
        assert_eq!(claim.owner_session.as_ref(), Some(&claude));
        let alive = |_pid: u32| true;
        let pane = |file: &Path| {
            agent_doc_turn_status_io::pane_harness_session_for_file(file, "%claimliveness")
        };

        // Before any hook: nothing contradicts the owner session.
        assert_eq!(
            claim_owner_liveness_observed(&claim, None, pane(&doc), alive),
            ClaimOwnerLiveness::Live
        );
        // The pane's `/clear` SessionStart hook reports the new session.
        agent_doc_turn_status_io::apply_turn_status_hook_at(
            dir.path(),
            "%claimliveness",
            false,
            Some(
                r#"{"hook_event_name":"SessionStart","source":"clear","session_id":"post-clear"}"#,
            ),
            now + 1,
        )
        .unwrap();
        assert_eq!(
            claim_owner_liveness_observed(&claim, None, pane(&doc), alive),
            ClaimOwnerLiveness::Ended(
                agent_doc_queue::queue_claim::OwnerSessionEndReason::SessionReplaced
            )
        );
        // The claiming harness process is gone.
        assert_eq!(
            claim_owner_liveness_observed(&claim, None, None, |_pid| false),
            ClaimOwnerLiveness::Ended(
                agent_doc_queue::queue_claim::OwnerSessionEndReason::OwnerProcessExited
            )
        );
        // Adoption by the post-clear session.
        let adopted = mutate_ledger(&doc, |ledger| {
            ledger
                .start(
                    "#cligenport",
                    "subagent:cligoshell-then-cligenport",
                    "worker:cligenport",
                    now + 1,
                    7200,
                    Some(1),
                )
                .unwrap();
            Ok(ledger.refresh_in_session(
                "#cligenport",
                "subagent:cligoshell-then-cligenport",
                now + 2,
                7200,
                Some(ClaimOwnerSession {
                    session_id: Some("post-clear".to_string()),
                    ..claude.clone()
                }),
            ))
        })
        .unwrap()
        .unwrap();
        assert_eq!(
            claim_owner_liveness_observed(&adopted, None, pane(&doc), alive),
            ClaimOwnerLiveness::Live
        );
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }

    /// `#idlessqueueidentity`: once the operator deletes `id=` from the
    /// document's one `agent:run` and one `agent:loop`, `queue claim` must
    /// still find the run's unclaimed free-text rows (it used to report
    /// "live heads: (none)"), and a claim taken while the ids were present is
    /// still found afterwards: claims are keyed by work identity, never by
    /// queue id.
    #[test]
    fn claims_resolve_after_the_run_and_loop_ids_are_deleted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        let render = |run_id: &str, loop_id: &str| {
            format!(
                "---\nsession: sid\nagent_doc_format: template\nqueue: go\n---\n\n\
## Run <!-- agent:run {run_id}subagents priority go -->\n\
- `/agent-doc` without arguments matches `/agent-doc <document-path>`\n\
- Why have the ☑️ run items not completed?\n\
<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop {loop_id}interval=\"1h\" priority -->\n\
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues.\n\
<!-- /agent:loop -->\n"
            )
        };
        std::fs::write(&doc, render("id=\"main\" ", "id=\"issues\" ")).unwrap();
        let earlier = "`/agent-doc` without arguments matches `/agent-doc <document-path>`";
        let (outcome, _) = claim(&doc, earlier, "subagent:noargparity", 600).unwrap();
        assert_eq!(outcome, ClaimOutcome::Created);

        let idless = render("", "");
        std::fs::write(&doc, &idless).unwrap();
        assert!(
            claimed_items_for_content(&doc, &idless).claims(earlier),
            "a claim taken under `id=main` must survive the id deletion"
        );
        let free_text = "Why have the ☑️ run items not completed?";
        let (outcome, stored) = claim(&doc, free_text, "subagent:whyrun", 600)
            .unwrap_or_else(|err| panic!("id-less run head must be claimable: {err:#}"));
        assert_eq!(outcome, ClaimOutcome::Created);
        assert_eq!(stored.owner, "subagent:whyrun");
        assert!(claimed_items_for_content(&doc, &idless).claims(free_text));
    }

    #[test]
    fn claim_release_and_closure_round_trip_through_state_db() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["fix issue 109 now", "fix issue 110 now"]);
        let content = std::fs::read_to_string(&doc).unwrap();

        let miss = claim(&doc, "not in the queue", "subagent:x", 60).unwrap_err();
        assert!(
            miss.downcast_ref::<agent_doc_queue::queue_claim::QueueClaimMiss>()
                .is_some(),
            "claiming a non-head must fail loudly with a typed miss: {miss:#}"
        );
        assert!(
            format!("{miss}").starts_with(
                "no live queue head matches \"not in the queue\"; live heads: \"fix issue 109 now\""
            ),
            "{miss}"
        );
        let (outcome, stored) = claim(&doc, "🚧 fix issue 109 now", "subagent:gh109", 60).unwrap();
        assert_eq!(outcome, ClaimOutcome::Created);
        assert_eq!(stored.owner, "subagent:gh109");
        assert!(claimed_items_for_content(&doc, &content).claims("fix issue 109 now"));
        assert!(!claimed_items_for_content(&doc, &content).claims("fix issue 110 now"));

        // The item closes: it leaves the queue, so its claim no longer applies
        // and closeout reconciliation prunes it.
        let closed = content.replace("- fix issue 109 now\n", "");
        assert!(active_claims_for_content(&doc, &closed).unwrap().is_empty());
        assert_eq!(prune_closed_claims(&doc, &closed).unwrap(), 1);
        assert!(load_ledger(&doc).unwrap().claims.is_empty());

        claim(&doc, "fix issue 110 now", "subagent:gh110", 60).unwrap();
        assert!(release(&doc, "fix issue 110 now").unwrap().is_some());
        assert!(!claimed_items_for_content(&doc, &content).claims("fix issue 110 now"));
        assert!(release(&doc, "fix issue 110 now").unwrap().is_none());
    }

    /// `#claimfollowstruck`: strike-then-next-head through the durable
    /// ledger. A claim on the only `agent:run` row is struck, the strike
    /// drains `agent:run` (the loop becomes the selected queue), then a new
    /// `agent:run` head arrives. No ledger read or mutation carries the claim
    /// onto the loop row or the new head.
    #[test]
    fn a_struck_claim_stays_off_the_loop_row_and_the_next_run_head() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        let write = |run: &str, loop_rows: &str| {
            std::fs::write(
                &doc,
                format!(
                    "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Run <!-- agent:run go -->\n{run}<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop interval=\"1h\" -->\n{loop_rows}<!-- /agent:loop -->\n"
                ),
            )
            .unwrap();
            std::fs::read_to_string(&doc).unwrap()
        };
        let work = "port the windowed flush-prepare step";
        let watch = "- Watch https://x.test/issues for new issues\n";
        let next = "#gh-fix https://x.test/issues/380";
        write(&format!("- {work}\n"), watch);
        claim(&doc, work, "subagent:kotlinflush", 3600).unwrap();

        let drained = write(&format!("- ~~{work}~~\n"), watch);
        assert!(
            !ledger_claimed_items_for_content(&doc, &drained)
                .claims("Watch https://x.test/issues for new issues")
        );
        let arrived = write(&format!("- ~~{work}~~\n- {next}\n"), watch);
        assert!(
            !ledger_claimed_items_for_content(&doc, &arrived).claims(next),
            "the next head is dispatchable"
        );
        let following = load_ledger_following(&doc, &arrived).unwrap();
        assert_eq!(
            following.claims[0].item_text, work,
            "{:?}",
            following.claims
        );
        // A mutation persists the follow pass; the claim never moved.
        assert_eq!(prune_closed_claims(&doc, &arrived).unwrap(), 1);
        let ledger = load_ledger(&doc).unwrap();
        assert!(ledger.claims.is_empty(), "{:?}", ledger.claims);
    }

    /// `#claimclosureallqueues`: a claim's closure is judged against its own
    /// queue, never the selected one. A loop row is claimed while
    /// `agent:loop` is the selected queue; then `agent:run` gains work and
    /// becomes the selection. The loop row is still live, so every durable
    /// reader keeps its claim and closeout prune leaves it. Striking the loop
    /// row still closes it.
    #[test]
    fn a_live_loop_row_claim_survives_while_agent_run_is_selected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        let write = |run: &str, loop_rows: &str| {
            std::fs::write(
                &doc,
                format!(
                    "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Run <!-- agent:run go -->\n{run}<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop interval=\"1h\" -->\n{loop_rows}<!-- /agent:loop -->\n"
                ),
            )
            .unwrap();
            std::fs::read_to_string(&doc).unwrap()
        };
        let watch = "Watch https://x.test/issues for new issues";
        let next = "#gh-fix https://x.test/issues/380";
        write("", &format!("- {watch}\n"));
        claim(&doc, watch, "subagent:loopwatch", 3600).unwrap();

        let arrived = write(&format!("- {next}\n"), &format!("- {watch}\n"));
        assert_eq!(
            live_queue_head_texts(&arrived).unwrap(),
            vec![next.to_string()],
            "precondition: agent:run is the selected queue"
        );
        assert!(
            ledger_claimed_items_for_content(&doc, &arrived).claims(watch),
            "the claimed loop row is never re-offered"
        );
        assert!(!ledger_claimed_items_for_content(&doc, &arrived).claims(next));
        let active = active_claims_for_content(&doc, &arrived).unwrap();
        assert_eq!(active.len(), 1, "{active:?}");
        assert_eq!(active[0].item_text, watch);
        assert_eq!(claim_states_for_content(&doc, &arrived).unwrap().len(), 1);
        assert_eq!(prune_closed_claims(&doc, &arrived).unwrap(), 0);
        assert_eq!(load_ledger(&doc).unwrap().claims.len(), 1);

        // The inverse: a struck loop row closes its claim, run still selected.
        let struck = write(&format!("- {next}\n"), &format!("- ~~{watch}~~\n"));
        assert!(active_claims_for_content(&doc, &struck).unwrap().is_empty());
        assert_eq!(prune_closed_claims(&doc, &struck).unwrap(), 1);
        assert!(load_ledger(&doc).unwrap().claims.is_empty());
    }

    #[test]
    fn followed_claim_target_does_not_fall_back_to_stale_disk_when_authority_is_live() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["rename this queue item"]);
        claim(&doc, "rename this queue item", "subagent:worker", 600).unwrap();
        let content = std::fs::read_to_string(&doc).unwrap();
        let disk_text = content.replace(
            "rename this queue item",
            "rename this queue item with context",
        );
        let disk_heads = live_queue_head_rows(&disk_text).unwrap();
        let disk_view = ClaimQueueView::from_content(&disk_text).unwrap();
        let authority_text = content.replace("- rename this queue item\n", "");
        let authority_heads = live_queue_head_rows(&authority_text).unwrap_or_default();
        let authority_view = ClaimQueueView::from_content(&authority_text).unwrap();
        assert_eq!(
            followed_claim_target(
                &doc,
                "rename this queue item",
                (&disk_heads, &disk_view),
                None,
            )
            .map(|(target, _)| target),
            Some("rename this queue item with context".to_string()),
            "the disk view alone does resolve the followed edit"
        );

        assert_eq!(
            followed_claim_target(
                &doc,
                "rename this queue item",
                (&disk_heads, &disk_view),
                Some((&authority_heads, &authority_view)),
            ),
            None
        );
    }

    #[test]
    fn start_persists_followed_edit_before_addressing_current_claim_identity() {
        let dir = tempfile::tempdir().unwrap();
        let original = "Run Agent Doc on sdk-demo.md (Grok)";
        let edited = "Run Agent Doc on sdk-demo.md (Grok): line 229 is not correct";
        let doc = write_doc(dir.path(), &[original]);
        let (_, claimed) = claim(&doc, original, "subagent:run-sdk-demo-grok", 600).unwrap();
        let current = std::fs::read_to_string(&doc)
            .unwrap()
            .replace(original, edited);
        std::fs::write(&doc, current).unwrap();

        let started = start_attempt(
            &doc,
            original,
            "subagent:run-sdk-demo-grok",
            "worker:run-sdk-demo-grok",
            600,
            claimed.generation,
        )
        .expect("the original dispatch text must resolve through the followed edit");

        assert_eq!(started.item_text, edited);
        assert_eq!(started.followed_from, [original]);
        assert_eq!(
            started.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert_eq!(
            start_attempt(
                &doc,
                edited,
                "subagent:run-sdk-demo-grok",
                "worker:run-sdk-demo-grok",
                600,
                claimed.generation,
            )
            .expect("the current exact item text must replay idempotently")
            .worker
            .unwrap()
            .worker_id,
            "worker:run-sdk-demo-grok",
        );
        let durable = load_ledger(&doc).unwrap().claims.remove(0);
        assert_eq!(durable.item_text, edited);
        assert_eq!(durable.followed_from, [original]);
    }

    #[test]
    fn durable_completion_evidence_recovers_only_the_exact_generation() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["finish worker task"]);
        let (_, claimed) = claim(&doc, "finish worker task", "subagent:worker", 600).unwrap();
        let claim = start_attempt(
            &doc,
            "finish worker task",
            "subagent:worker",
            "worker:exact",
            600,
            claimed.generation,
        )
        .unwrap();

        let mut stale = claim.clone();
        stale.generation = 0;
        let stale_marker = agent_doc_document::subagent_completion::marker(
            &completion_feedback_key(&stale).unwrap(),
        );
        assert_eq!(
            reconcile_completion_evidence(&doc, &stale_marker).unwrap(),
            0,
            "an unfenced historical result must not attest generation one"
        );

        let mut foreign = claim.clone();
        foreign.worker.as_mut().unwrap().worker_id = "worker:foreign".to_string();
        let foreign_marker = agent_doc_document::subagent_completion::marker(
            &completion_feedback_key(&foreign).unwrap(),
        );
        assert_ne!(
            completion_feedback_key(&claim).unwrap(),
            completion_feedback_key(&foreign).unwrap(),
            "the worker identity is part of exact-attempt attestation"
        );
        assert_eq!(
            reconcile_completion_evidence(&doc, &foreign_marker).unwrap(),
            0,
            "a foreign worker bound to the same owner and generation cannot report"
        );

        let mut unstarted = claim.clone();
        unstarted.worker = None;
        assert!(
            completion_feedback_key(&unstarted).is_err(),
            "an unstarted generated claim cannot manufacture completion evidence"
        );

        let exact = agent_doc_document::subagent_completion::cell(
            &claim.owner,
            &claim.item_text,
            &completion_feedback_key(&claim).unwrap(),
        );
        assert_eq!(reconcile_completion_evidence(&doc, &exact).unwrap(), 1);
        let stored = load_ledger(&doc).unwrap().claims.remove(0);
        assert_eq!(stored.generation, 1);
        assert_eq!(
            stored.state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
        assert!(stored.blocks_dispatch(u64::MAX));
    }

    fn refusal(err: &anyhow::Error) -> &agent_doc_queue::queue_claim::RefreshRefusal {
        &err.downcast_ref::<QueueClaimRefreshRefused>()
            .unwrap_or_else(|| panic!("expected a typed refresh refusal: {err:#}"))
            .reason
    }

    /// `queue claim --refresh` (qsubattr3): the owner's live claim is
    /// extended in state.db; an unclaimed, expired, or foreign claim is refused
    /// without creating or moving a claim.
    #[test]
    fn refresh_extends_the_owners_live_claim_and_refuses_otherwise() {
        use agent_doc_queue::queue_claim::RefreshRefusal;
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#a]", "do [#b]"]);

        let unclaimed = refresh(&doc, "#a", "subagent:a", 600).unwrap_err();
        assert_eq!(refusal(&unclaimed), &RefreshRefusal::Unclaimed);
        assert!(
            load_ledger(&doc).unwrap().claims.is_empty(),
            "a refused refresh must not create a claim"
        );

        let (_, claimed) = claim(&doc, "#a", "subagent:a", 60).unwrap();
        start_attempt(&doc, "#a", "subagent:a", "worker:a", 60, claimed.generation).unwrap();
        let refreshed = refresh(&doc, "#a", "subagent:a", 7200).unwrap();
        assert!(
            refreshed.expires_at_secs >= claimed.expires_at_secs + 7000,
            "{claimed:?} -> {refreshed:?}"
        );
        assert_eq!(refreshed.claimed_at_secs, claimed.claimed_at_secs);
        assert_eq!(load_ledger(&doc).unwrap().claims[0], refreshed);

        let foreign = refresh(&doc, "do [#a]", "subagent:b", 600).unwrap_err();
        assert_eq!(
            refusal(&foreign),
            &RefreshRefusal::OtherOwner {
                holder: "subagent:a".to_string()
            }
        );
        assert_eq!(load_ledger(&doc).unwrap().claims[0], refreshed);

        // Judged past its expiry, the same owner's refresh is refused and the
        // stored claim is left as it was (not revived).
        let expired =
            refresh_at(&doc, "#a", "subagent:a", 600, refreshed.expires_at_secs).unwrap_err();
        assert_eq!(refusal(&expired).kind(), "expired");
        assert_eq!(load_ledger(&doc).unwrap().claims[0], refreshed);

        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops.contains("queue_claim_refresh outcome=refreshed owner=subagent:a"),
            "{ops}"
        );
        assert!(
            ops.contains("queue_claim_refresh outcome=refused reason=other_owner"),
            "{ops}"
        );
    }

    #[test]
    fn start_binds_exactly_one_worker_to_the_exact_generation() {
        use agent_doc_queue::queue_claim::QueueClaimStartRefused;

        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["do [#a]", "do [#b]"]);
        let (_, a) = claim(&doc, "#a", "subagent:a", 60).unwrap();
        let (_, b) = claim(&doc, "#b", "subagent:b", 60).unwrap();

        let stale = start_attempt(
            &doc,
            "#a",
            "subagent:a",
            "worker:one",
            600,
            a.generation + 1,
        )
        .unwrap_err();
        assert!(
            matches!(
                stale.downcast_ref::<QueueClaimStartRefused>(),
                Some(QueueClaimStartRefused::StaleAttempt { .. })
            ),
            "{stale:#}"
        );

        let running =
            start_attempt(&doc, "#a", "subagent:a", "worker:one", 600, a.generation).unwrap();
        assert_eq!(
            running.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert_eq!(
            running
                .worker
                .as_ref()
                .map(|worker| worker.worker_id.as_str()),
            Some("worker:one")
        );
        assert_eq!(
            start_attempt(&doc, "#a", "subagent:a", "worker:one", 600, a.generation,).unwrap(),
            running
        );

        let rebound =
            start_attempt(&doc, "#a", "subagent:a", "worker:two", 600, a.generation).unwrap_err();
        assert!(matches!(
            rebound.downcast_ref::<QueueClaimStartRefused>(),
            Some(QueueClaimStartRefused::AttemptAlreadyBound { .. })
        ));
        let reused =
            start_attempt(&doc, "#b", "subagent:b", "worker:one", 600, b.generation).unwrap_err();
        assert!(matches!(
            reused.downcast_ref::<QueueClaimStartRefused>(),
            Some(QueueClaimStartRefused::WorkerAlreadyBound { .. })
        ));

        let stored = load_ledger(&doc).unwrap();
        assert_eq!(
            stored
                .claims
                .iter()
                .filter(|claim| claim.worker.is_some())
                .count(),
            1
        );
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(ops.contains("queue_claim_start outcome=running"), "{ops}");
        assert!(ops.contains("queue_claim_start outcome=refused"), "{ops}");
    }

    #[test]
    fn overdue_unstarted_claim_is_an_actionable_dispatch_obligation() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(dir.path(), &["#subagents do [#gh301]"]);
        let content = std::fs::read_to_string(&doc).unwrap();
        let (_, claim) = claim(&doc, "#gh301", "subagent:gh301", 600).unwrap();

        assert!(
            dispatch_acknowledgement_obligations_for_content_at(
                &doc,
                &content,
                claim.claimed_at_secs + 9,
                10,
            )
            .unwrap()
            .is_empty()
        );
        let obligations = dispatch_acknowledgement_obligations_for_content_at(
            &doc,
            &content,
            claim.claimed_at_secs + 10,
            10,
        )
        .unwrap();
        assert_eq!(obligations.len(), 1);
        let obligation = &obligations[0];
        assert_eq!(obligation.item, "#subagents do [#gh301]");
        assert!(
            obligation.start_command.contains(
                "--item '#gh301' --owner 'subagent:gh301' --generation 1 --worker-id <worker-id>"
            ),
            "{}",
            obligation.start_command,
        );
        assert!(
            obligation
                .release_command
                .contains("--item '#gh301' --generation 1 --requeue"),
            "{}",
            obligation.release_command,
        );

        start_attempt(
            &doc,
            "#gh301",
            "subagent:gh301",
            "worker:gh301",
            600,
            claim.generation,
        )
        .unwrap();
        assert!(
            dispatch_acknowledgement_obligations_for_content_at(
                &doc,
                &content,
                claim.claimed_at_secs + 100,
                10,
            )
            .unwrap()
            .is_empty(),
            "a worker-bound Running attempt is genuinely in flight"
        );
    }

    #[test]
    fn claim_by_hash_id_targets_a_preset_prefixed_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &[
                "#gh-fix https://x/issues/110",
                "#subagents do [#preflightdeadline]",
            ],
        );
        let content = std::fs::read_to_string(&doc).unwrap();
        let (_, stored) = claim(&doc, "#preflightdeadline", "subagent:pd", 60).unwrap();
        assert_eq!(stored.item_text, "#subagents do [#preflightdeadline]");
        assert!(
            claimed_items_for_content(&doc, &content).claims("#subagents do [#preflightdeadline]"),
            "the drainability filter must see the claim on the full head"
        );
        assert!(release(&doc, "#preflightdeadline").unwrap().is_some());
        assert!(load_ledger(&doc).unwrap().claims.is_empty());
    }

    /// GH #304: a lifecycle marker projected onto a requeued head is runtime
    /// state, not operator-authored claim text. Both id-backed and free-text
    /// claims must store and return the marker-free head.
    #[test]
    fn claim_strips_projected_lifecycle_marker_from_stored_text() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            &["⚠️ do [#upgrade]", "🚧 finish the free-text task"],
        );

        let (_, id_claim) = claim(&doc, "#upgrade", "subagent:upgrade", 60).unwrap();
        assert_eq!(id_claim.item_text, "do [#upgrade]");

        let (_, text_claim) =
            claim(&doc, "finish the free-text task", "subagent:free-text", 60).unwrap();
        assert_eq!(text_claim.item_text, "finish the free-text task");

        let ledger = load_ledger(&doc).unwrap();
        assert_eq!(
            ledger
                .claims
                .iter()
                .map(|claim| claim.item_text.as_str())
                .collect::<Vec<_>>(),
            vec!["do [#upgrade]", "finish the free-text task"]
        );

        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.lines()
                .any(|line| line.contains("queue_claim") && line.contains("item_bytes=13")),
            "the claim log measures canonical stored text, not the shorter #id argument: {ops}"
        );
    }
}

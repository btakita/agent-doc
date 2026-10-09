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
    ClaimHeadSource, ClaimOutcome, ClaimedQueueItems, QueueClaim, QueueClaimLedger, QueueClaimMiss,
    QueueClaimRefreshRefused, claim_identity, resolve_claim_target_in_views,
};
use agent_doc_queue::queue_continuation::{live_queue_head_identities, live_queue_head_texts};
use anyhow::{Context, Result, bail};

use agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
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
            && let Some(heads) = std::fs::read_to_string(file)
                .ok()
                .as_deref()
                .and_then(live_queue_head_texts)
        {
            log_followed(file, &ledger.follow_edits(now_secs(), &heads));
        }
        let value = mutate(&mut ledger)?;
        if ledger.claims.is_empty() {
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

fn log_followed(file: &Path, followed: &[(String, String)]) {
    for (previous, current) in followed {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_followed_edit previous_bytes={} current_bytes={} (#claimfollowsedit)",
                previous.len(),
                current.len()
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
        && let Some(heads) = live_queue_head_texts(content)
    {
        ledger.follow_edits(now_secs(), &heads);
    }
    Ok(ledger)
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

/// Live queue heads of the editor/CRDT authority's current text, the view
/// preflight computes `queue_subagent_dispatch` from. `None` when no editor
/// owns the document (disk is the authority) or no current cut is available.
fn editor_authority_queue_heads(file: &Path, source: &str) -> Option<Vec<String>> {
    live_queue_head_texts(&editor_authority_text(file, source)?)
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

/// Resolve a claim `--item` against the on-disk document, falling back to
/// the editor authority's current text on a miss (GH #124 ask 5): dispatch
/// and claim must agree on what a live head is even while an editor save to
/// disk is deferred.
fn resolve_live_claim_target(file: &Path, item: &str, source: &str) -> Result<String> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("read {} to resolve queue item {item:?}", file.display()))?;
    let disk_heads = live_queue_head_texts(&content).unwrap_or_default();
    let resolved = match resolve_claim_target_in_views(item, &disk_heads, None) {
        Ok(resolved) => resolved,
        Err(_) => {
            let authority = editor_authority_queue_heads(file, source);
            resolve_claim_target_in_views(item, &disk_heads, authority.as_deref())?
        }
    };
    if resolved.1 == ClaimHeadSource::EditorAuthority {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_resolved source={} reason=head_not_yet_saved_to_disk item_bytes={}",
                resolved.1.as_str(),
                item.trim().len()
            ),
        );
    }
    Ok(resolved.0)
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
    if item.trim().is_empty() {
        bail!("queue claim needs a non-empty --item (`#id` or the queue line's text)");
    }
    if owner.trim().is_empty() {
        bail!("queue claim needs a non-empty --owner (e.g. `subagent:<label>`)");
    }
    // A miss is a typed usage error (`QueueClaimMiss`) naming the live heads,
    // never a bare string: the CLI reports it as-is instead of wrapping it in
    // the generic turn-failure notice.
    let target = resolve_live_claim_target(file, item, "queue_claim")?;
    let identity = claim_identity(&target);
    let now = now_secs();
    let result = mutate_ledger(file, |ledger| {
        let outcome = ledger.claim(&target, owner, now, ttl_secs);
        let stored = ledger
            .claims
            .iter()
            .find(|claim| claim.identity == identity)
            .cloned()
            .context("claimed item missing from the ledger after claim")?;
        Ok((outcome, stored))
    })?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_claim outcome={:?} owner={} expires_at={} item_bytes={}",
            result.0,
            result.1.owner,
            result.1.expires_at_secs,
            item.trim().len()
        ),
    );
    Ok(result)
}

/// Extend `owner`'s live claim on `item` to `now + ttl_secs`
/// (`queue claim --refresh`). Refuses, with a typed
/// [`QueueClaimRefreshRefused`], when the item is unclaimed, the claim already
/// expired, or another owner holds it; it never creates a claim.
pub fn refresh(file: &Path, item: &str, owner: &str, ttl_secs: u64) -> Result<QueueClaim> {
    refresh_at(file, item, owner, ttl_secs, now_secs())
}

fn refresh_at(file: &Path, item: &str, owner: &str, ttl_secs: u64, now: u64) -> Result<QueueClaim> {
    if item.trim().is_empty() {
        bail!("queue claim --refresh needs a non-empty --item (`#id` or the queue line's text)");
    }
    if owner.trim().is_empty() {
        bail!("queue claim --refresh needs a non-empty --owner (the claim's holder)");
    }
    // Same resolution as `claim`, so `--item #id` refreshes a
    // `#subagents do [#id]` head and a closed head is a typed miss.
    let target = resolve_live_claim_target(file, item, "queue_claim_refresh")?;
    let outcome = mutate_ledger(file, |ledger| {
        Ok(ledger.refresh(&target, owner, now, ttl_secs))
    })?;
    match &outcome {
        Ok(claim) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_claim_refresh outcome=refreshed owner={} expires_at={} item_bytes={}",
                claim.owner,
                claim.expires_at_secs,
                item.trim().len()
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

/// Release the claim on `item`. Returns the released claim, if there was one.
pub fn release(file: &Path, item: &str) -> Result<Option<QueueClaim>> {
    // Release by the same resolution `claim` used, so `--item #id` releases a
    // `#subagents do [#id]` head; fall back to the literal item (a closed head
    // can still hold a stale claim worth releasing).
    let target = match resolve_live_claim_target(file, item, "queue_claim_release") {
        Ok(target) => target,
        Err(err) => {
            if err.downcast_ref::<QueueClaimMiss>().is_none() {
                eprintln!(
                    "[queue-claim] WARNING: could not resolve {item:?} in {}; releasing it literally: {err:#}",
                    file.display()
                );
            }
            item.to_string()
        }
    };
    let released = mutate_ledger(file, |ledger| {
        Ok(ledger.release(&target).or_else(|| ledger.release(item)))
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

/// Active claims for `file` judged against `content`: expired claims and claims
/// on items no longer in the queue are excluded.
pub fn active_claims_for_content(file: &Path, content: &str) -> Result<Vec<QueueClaim>> {
    let ledger = load_ledger_following(file, content)?;
    if ledger.claims.is_empty() {
        return Ok(Vec::new());
    }
    let live = live_queue_head_identities(content);
    Ok(ledger
        .active(now_secs(), live.as_ref())
        .into_iter()
        .cloned()
        .collect())
}

/// The claimed heads the in-session drainability filter must skip.
///
/// A ledger that cannot be read is reported and treated as "no claims": the
/// failure direction is the pre-claim behaviour (the head stays drainable),
/// never a stranded queue.
pub fn claimed_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    let claimed = ledger_claimed_items_for_content(file, content);
    // The queue-level subagents attribute holds heads back (cap full, or an
    // `after=` predecessor still queued). A held head is out of the in-session
    // loop exactly like a claimed one: never drained inline, never counted.
    match crate::subagent_dispatch::queue_attr_subagent_plan(file, content) {
        Some(plan) if !plan.held.is_empty() => claimed.with_heads(&plan.held),
        _ => claimed,
    }
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

/// Only the worker claims recorded in the ledger, without heads the queue
/// subagents attribute holds back.
pub fn ledger_claimed_items_for_content(file: &Path, content: &str) -> ClaimedQueueItems {
    match load_ledger_following(file, content) {
        Ok(ledger) if ledger.claims.is_empty() => ClaimedQueueItems::none(),
        Ok(ledger) => {
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
    let live = live_queue_head_identities(content);
    let heads = live_queue_head_texts(content);
    let now = now_secs();
    let pruned = mutate_ledger(file, |ledger| {
        // Follow against the closeout's content (the disk read inside
        // `mutate_ledger` may lag it) before closure is judged.
        if let Some(heads) = &heads {
            log_followed(file, &ledger.follow_edits(now, heads));
        }
        Ok(ledger.prune(now, live.as_ref()))
    })?;
    if pruned > 0 {
        agent_doc_ops_log_io::log_op(file, &format!("queue_claim_prune pruned={pruned}"));
    }
    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

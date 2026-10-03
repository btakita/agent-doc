//! Pure queue-item claim policy (`#queueclaim`).
//!
//! A queue head that has been handed to a worker outside the in-session loop —
//! a background subagent under a `#subagents` preset, for example — is
//! **claimed**: the work is already in flight elsewhere, so it is not drainable
//! by the in-session loop. Without a claim, the Stop hook saw the dispatched
//! head as the next "drainable head" and forced a `/loop` re-entry while the
//! parent turn was only waiting on its subagents. The re-entry then either
//! churned a no-op cycle or consumed the queue item before the work landed.
//! Observed 2026-10-02 on tasks/agent-doc/agent-doc-bugs.md (`#gh-fix` heads
//! for GH #109/#110/#111 dispatched to subagents).
//!
//! # Identity
//!
//! A claim keys on [`queue_head_identity`], the queue head's **work
//! identity** that dispatch, mid-turn steering, drainability and preflight
//! selection share (`#claimdispatchidentity`): `#id` for id-backed heads,
//! otherwise the normalized text left after lifecycle markers (`🚧`, `⏭️`,
//! pins) and leading intent/preset tags (`#subagents`, `#subagent:`,
//! `#gh-fix`) are stripped. So `🚧 do [#a]`, `#subagents do [#a]` and `[#a]`
//! are one claim, and an operator edit of a claimed
//! `#subagents: <url>` into `#subagents: #gh-fix <url>` keeps its claim
//! instead of re-appearing as new, unclaimed dispatch work. Changing the
//! substance — a different `#id`, URL or prose — is a retarget and does not
//! inherit the claim.
//!
//! # Lifetime
//!
//! A claim ends on whichever comes first:
//!
//! * **closure** — the item is no longer a live head in the queue (it was
//!   answered and struck, reaped, or deleted by the operator). A claim on an
//!   item that no longer exists is meaningless, so it is ignored on read and
//!   pruned at closeout reconciliation;
//! * **explicit release** — `agent-doc queue release` when the worker reports
//!   back, so the in-session loop can take the item again and close it;
//! * **expiry** — a TTL ([`DEFAULT_QUEUE_CLAIM_TTL_SECS`], overridable per
//!   claim). A background subagent has no pid or heartbeat agent-doc can
//!   observe, so a crashed worker can never release its claim. Without a TTL a
//!   lost subagent would strand the head forever; with one, the queue degrades
//!   back to ordinary drainability. `queue claim --refresh`
//!   ([`QueueClaimLedger::refresh`]) extends the TTL of the owner's own live
//!   claim, which lets a long-running owner keep its claim alive; it refuses an
//!   expired, foreign, or missing claim rather than (re)creating one. Under the
//!   queue-level `subagents` attribute an expired claim returns the head to the
//!   dispatch set, never to inline drainage.
//!
//! This module is pure: no clock, no storage. The caller supplies `now`.

use std::collections::HashSet;

use agent_doc_element_queue::{QueueItemIdentity, queue_head_identity};
use serde::{Deserialize, Serialize};

/// Default claim TTL: two hours. Long enough for a background subagent to fix
/// and verify one issue; short enough that a crashed one strands the queue for
/// a bounded, visible interval rather than indefinitely.
pub const DEFAULT_QUEUE_CLAIM_TTL_SECS: u64 = 2 * 60 * 60;

/// One durable claim on a queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueClaim {
    pub identity: QueueItemIdentity,
    /// The item text as given when claimed, for diagnostics.
    pub item_text: String,
    /// Who holds the claim, e.g. `subagent:gh109`.
    pub owner: String,
    pub claimed_at_secs: u64,
    pub expires_at_secs: u64,
}

impl QueueClaim {
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs >= self.expires_at_secs
    }
}

/// The document's claim ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueClaimLedger {
    #[serde(default)]
    pub claims: Vec<QueueClaim>,
}

/// Outcome of [`QueueClaimLedger::claim`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimOutcome {
    Created,
    /// The same owner re-claimed the item; its TTL was refreshed.
    Refreshed,
    /// A different owner held a live claim; ownership moved to the new owner.
    Reassigned,
}

impl QueueClaimLedger {
    /// Re-derive every stored identity from its `item_text` with the current
    /// [`claim_identity`], merging claims that now share one identity (the
    /// later expiry wins). A ledger written before the identity rule changed
    /// (tag-sensitive free-text keys) would otherwise hold claims that match
    /// no live head and silently drop. Returns whether anything changed.
    pub fn rekey(&mut self) -> bool {
        let mut changed = false;
        let mut merged: Vec<QueueClaim> = Vec::with_capacity(self.claims.len());
        for mut claim in std::mem::take(&mut self.claims) {
            let identity = claim_identity(&claim.item_text);
            if identity != claim.identity {
                claim.identity = identity;
                changed = true;
            }
            match merged
                .iter_mut()
                .find(|kept| kept.identity == claim.identity)
            {
                Some(kept) => {
                    changed = true;
                    if claim.expires_at_secs > kept.expires_at_secs {
                        *kept = claim;
                    }
                }
                None => merged.push(claim),
            }
        }
        self.claims = merged;
        changed
    }

    /// Claim `item` for `owner` until `now_secs + ttl_secs`.
    pub fn claim(&mut self, item: &str, owner: &str, now_secs: u64, ttl_secs: u64) -> ClaimOutcome {
        let identity = claim_identity(item);
        let expires_at_secs = now_secs.saturating_add(ttl_secs.max(1));
        let fresh = QueueClaim {
            identity: identity.clone(),
            item_text: item.trim().to_string(),
            owner: owner.trim().to_string(),
            claimed_at_secs: now_secs,
            expires_at_secs,
        };
        match self
            .claims
            .iter_mut()
            .find(|claim| claim.identity == identity)
        {
            Some(existing) => {
                let outcome = if existing.owner == fresh.owner || existing.is_expired(now_secs) {
                    ClaimOutcome::Refreshed
                } else {
                    ClaimOutcome::Reassigned
                };
                *existing = fresh;
                outcome
            }
            None => {
                self.claims.push(fresh);
                ClaimOutcome::Created
            }
        }
    }

    /// Extend the TTL of `owner`'s live claim on `item` to `now_secs + ttl_secs`
    /// (`queue claim --refresh`, the coordinator's heartbeat for a long-running
    /// subagent). Unlike [`Self::claim`], it never creates or takes over a
    /// claim: an unclaimed item, an expired claim, or another owner's claim is
    /// refused, so a heartbeat that arrives after the claim lapsed cannot
    /// silently re-claim a head another worker may already have taken.
    /// `claimed_at_secs` is kept; only `expires_at_secs` moves.
    pub fn refresh(
        &mut self,
        item: &str,
        owner: &str,
        now_secs: u64,
        ttl_secs: u64,
    ) -> Result<QueueClaim, QueueClaimRefreshRefused> {
        let identity = claim_identity(item);
        let owner = owner.trim();
        let refused = |reason| QueueClaimRefreshRefused {
            item: item.trim().to_string(),
            owner: owner.to_string(),
            reason,
        };
        let Some(existing) = self
            .claims
            .iter_mut()
            .find(|claim| claim.identity == identity)
        else {
            return Err(refused(RefreshRefusal::Unclaimed));
        };
        if existing.is_expired(now_secs) {
            return Err(refused(RefreshRefusal::Expired {
                holder: existing.owner.clone(),
                expired_at_secs: existing.expires_at_secs,
            }));
        }
        if existing.owner != owner {
            return Err(refused(RefreshRefusal::OtherOwner {
                holder: existing.owner.clone(),
            }));
        }
        existing.expires_at_secs = now_secs.saturating_add(ttl_secs.max(1));
        Ok(existing.clone())
    }

    /// Release the claim on `item`. Returns the released claim, if any.
    pub fn release(&mut self, item: &str) -> Option<QueueClaim> {
        let identity = claim_identity(item);
        let index = self
            .claims
            .iter()
            .position(|claim| claim.identity == identity)?;
        Some(self.claims.remove(index))
    }

    /// Claims that are neither expired nor closed. `live_heads` is the set of
    /// identities still present as live queue heads; `None` means the caller
    /// could not read the queue, in which case closure cannot be judged and
    /// only expiry applies.
    pub fn active(
        &self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> Vec<&QueueClaim> {
        self.claims
            .iter()
            .filter(|claim| !claim.is_expired(now_secs))
            .filter(|claim| live_heads.is_none_or(|live| live.contains(&claim.identity)))
            .collect()
    }

    /// Drop expired and closed claims. Returns how many were removed.
    pub fn prune(
        &mut self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> usize {
        let before = self.claims.len();
        self.claims.retain(|claim| {
            !claim.is_expired(now_secs)
                && live_heads.is_none_or(|live| live.contains(&claim.identity))
        });
        before - self.claims.len()
    }

    /// The claimed identities a drainability computation must skip.
    pub fn claimed_items(
        &self,
        now_secs: u64,
        live_heads: Option<&HashSet<QueueItemIdentity>>,
    ) -> ClaimedQueueItems {
        ClaimedQueueItems(
            self.active(now_secs, live_heads)
                .into_iter()
                .map(|claim| claim.identity.clone())
                .collect(),
        )
    }
}

/// The identity a claim on `item` keys on: the shared queue-head work
/// identity ([`queue_head_identity`]). Every claim comparison — the ledger,
/// [`ClaimedQueueItems::claims`], live-head closure, dispatch membership —
/// goes through this one function.
pub fn claim_identity(item: &str) -> QueueItemIdentity {
    queue_head_identity(item.trim())
}

/// Tracked ids a queue line references as `[#id]` or `do #id`, lowercased, in
/// first-seen order. Preset tags (`#subagents`, `#gh-fix`) are not references,
/// so `#subagents do [#a]` references exactly `a`.
pub fn referenced_queue_ids(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let is_id_char = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_');
    let mut out: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        if !id.is_empty() && id.chars().all(is_id_char) && !out.iter().any(|seen| seen == id) {
            out.push(id.to_string());
        }
    };
    let mut rest = lower.as_str();
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        match after.find(']') {
            Some(end) => {
                push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    for pair in words.windows(2) {
        if pair[0] == "do"
            && let Some(id) = pair[1].strip_prefix('#')
        {
            let id: String = id.chars().take_while(|ch| is_id_char(*ch)).collect();
            push(&id);
        }
    }
    out
}

/// Why [`QueueClaimLedger::refresh`] refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshRefusal {
    /// No claim on the item at all.
    Unclaimed,
    /// The claim's TTL already lapsed; the head is back in the eligible set.
    Expired {
        holder: String,
        expired_at_secs: u64,
    },
    /// A different owner holds the live claim.
    OtherOwner { holder: String },
}

impl RefreshRefusal {
    /// Stable short name for ops-log lines.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unclaimed => "unclaimed",
            Self::Expired { .. } => "expired",
            Self::OtherOwner { .. } => "other_owner",
        }
    }
}

/// A `queue claim --refresh` that has no live claim of its owner to extend.
/// An operator/agent usage error with a specific remedy, not an Agent Doc
/// turn failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueClaimRefreshRefused {
    pub item: String,
    pub owner: String,
    pub reason: RefreshRefusal,
}

impl std::fmt::Display for QueueClaimRefreshRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.reason {
            RefreshRefusal::Unclaimed => write!(
                f,
                "cannot refresh {:?}: it is not claimed. Claim it first with `queue claim` \
                 (without --refresh).",
                self.item
            ),
            RefreshRefusal::Expired {
                holder,
                expired_at_secs,
            } => write!(
                f,
                "cannot refresh {:?}: the claim held by {holder} expired at {expired_at_secs}. \
                 The head is eligible for dispatch again; claim it anew (without --refresh) only \
                 if no other worker has taken it.",
                self.item
            ),
            RefreshRefusal::OtherOwner { holder } => write!(
                f,
                "cannot refresh {:?} for {}: the live claim is held by {holder}.",
                self.item, self.owner
            ),
        }
    }
}

impl std::error::Error for QueueClaimRefreshRefused {}

/// A `queue claim` / `queue release` `--item` that names no live queue head
/// (or names several). An operator/agent usage error with a specific remedy,
/// not an Agent Doc turn failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueClaimMiss {
    pub item: String,
    pub live_heads: Vec<String>,
    /// Several live heads reference the `#id`; empty for a plain miss.
    pub ambiguous: Vec<String>,
}

impl std::fmt::Display for QueueClaimMiss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quote = |items: &[String]| {
            items
                .iter()
                .map(|item| format!("{item:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if self.ambiguous.is_empty() {
            write!(
                f,
                "no live queue head matches {:?}; live heads: {}. Pass `--item` as a head's `#id` \
                 or its exact queue line text.",
                self.item,
                if self.live_heads.is_empty() {
                    "(none)".to_string()
                } else {
                    quote(&self.live_heads)
                }
            )
        } else {
            write!(
                f,
                "{:?} matches several live queue heads: {}. Pass `--item` as the exact queue line \
                 text.",
                self.item,
                quote(&self.ambiguous)
            )
        }
    }
}

impl std::error::Error for QueueClaimMiss {}

/// Resolve a claim `--item` to the live queue head it names.
///
/// The item matches a head by claim identity (the exact line, marker-
/// invariant, or a bare `#id` for an id-backed head). When that misses and the
/// item is an `#id`, it matches the single live head that REFERENCES the id,
/// so `#a` claims a `#subagents do [#a]` head whose identity is its full text.
pub fn resolve_claim_target(item: &str, live_heads: &[String]) -> Result<String, QueueClaimMiss> {
    let identity = claim_identity(item);
    if let Some(head) = live_heads
        .iter()
        .find(|head| claim_identity(head) == identity)
    {
        return Ok(head.clone());
    }
    if let QueueItemIdentity::Id(id) = &identity {
        let referencing: Vec<String> = live_heads
            .iter()
            .filter(|head| referenced_queue_ids(head).iter().any(|r| r == id))
            .cloned()
            .collect();
        match referencing.len() {
            1 => return Ok(referencing[0].clone()),
            0 => {}
            _ => {
                return Err(QueueClaimMiss {
                    item: item.trim().to_string(),
                    live_heads: live_heads.to_vec(),
                    ambiguous: referencing,
                });
            }
        }
    }
    Err(QueueClaimMiss {
        item: item.trim().to_string(),
        live_heads: live_heads.to_vec(),
        ambiguous: Vec::new(),
    })
}

/// Which document view a claim target resolved in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimHeadSource {
    /// The on-disk document.
    Disk,
    /// The live editor / CRDT authority text — the view preflight computes
    /// `queue_subagent_dispatch` from — while the editor's save to disk is
    /// still pending (e.g. a deferred native save).
    EditorAuthority,
}

impl ClaimHeadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disk => "disk",
            Self::EditorAuthority => "editor_authority",
        }
    }
}

/// [`resolve_claim_target`] across the two views of the document (GH #124
/// ask 5). Preflight computes `queue_subagent_dispatch` from the editor
/// authority's current text; `queue claim` used to resolve against disk only,
/// so a head the operator typed but the editor had not yet saved was offered
/// for dispatch and then refused as "no live queue head". The disk view wins
/// when both resolve; the authority view is consulted only on a disk miss. A
/// miss in both reports the disk miss, with the authority-only heads appended
/// to its live-head list so the operator sees every head either view holds.
pub fn resolve_claim_target_in_views(
    item: &str,
    disk_heads: &[String],
    authority_heads: Option<&[String]>,
) -> Result<(String, ClaimHeadSource), QueueClaimMiss> {
    let disk_miss = match resolve_claim_target(item, disk_heads) {
        Ok(target) => return Ok((target, ClaimHeadSource::Disk)),
        Err(miss) => miss,
    };
    let Some(authority_heads) = authority_heads else {
        return Err(disk_miss);
    };
    match resolve_claim_target(item, authority_heads) {
        Ok(target) => Ok((target, ClaimHeadSource::EditorAuthority)),
        Err(authority_miss) if !authority_miss.ambiguous.is_empty() => Err(authority_miss),
        Err(_) if !disk_miss.ambiguous.is_empty() => Err(disk_miss),
        Err(_) => {
            let mut live_heads = disk_miss.live_heads;
            for head in authority_heads {
                if !live_heads.contains(head) {
                    live_heads.push(head.clone());
                }
            }
            Err(QueueClaimMiss {
                item: disk_miss.item,
                live_heads,
                ambiguous: Vec::new(),
            })
        }
    }
}

/// A set of claimed queue items, consulted by the drainability filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimedQueueItems(HashSet<QueueItemIdentity>);

impl ClaimedQueueItems {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn from_identities(identities: impl IntoIterator<Item = QueueItemIdentity>) -> Self {
        Self(identities.into_iter().collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// These claims plus `heads` (by identity): the in-session exclusion set
    /// once the queue-level subagents attribute holds heads back.
    pub fn with_heads<'a>(mut self, heads: impl IntoIterator<Item = &'a String>) -> Self {
        self.0
            .extend(heads.into_iter().map(|head| claim_identity(head)));
        self
    }

    /// Whether the queue head `prompt_text` is claimed.
    pub fn claims(&self, prompt_text: &str) -> bool {
        !self.0.is_empty() && self.0.contains(&claim_identity(prompt_text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_marker_invariant_and_id_backed() {
        assert_eq!(claim_identity("🚧 do [#a]"), claim_identity("[#a]"));
        assert_eq!(
            claim_identity("📌 #gh-fix https://x/issues/109"),
            claim_identity("#gh-fix https://x/issues/109")
        );
        assert_ne!(
            claim_identity("#gh-fix https://x/issues/109"),
            claim_identity("#gh-fix https://x/issues/110")
        );
    }

    #[test]
    fn claim_refresh_reassign_and_release() {
        let mut ledger = QueueClaimLedger::default();
        assert_eq!(
            ledger.claim("do [#a]", "subagent:a", 100, 10),
            ClaimOutcome::Created
        );
        assert_eq!(
            ledger.claim("🚧 do [#a]", "subagent:a", 105, 10),
            ClaimOutcome::Refreshed
        );
        assert_eq!(ledger.claims[0].expires_at_secs, 115);
        assert_eq!(
            ledger.claim("[#a]", "subagent:b", 106, 10),
            ClaimOutcome::Reassigned
        );
        assert_eq!(ledger.claims.len(), 1);
        assert_eq!(ledger.release("do [#a]").unwrap().owner, "subagent:b");
        assert!(ledger.release("do [#a]").is_none());
    }

    #[test]
    fn refresh_extends_only_the_same_owners_live_claim() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("do [#a]", "subagent:a", 100, 10);

        // Same owner, live claim: the TTL moves, claimed_at does not.
        let refreshed = ledger.refresh("🚧 [#a]", "subagent:a", 105, 50).unwrap();
        assert_eq!(refreshed.expires_at_secs, 155);
        assert_eq!(refreshed.claimed_at_secs, 100);
        assert_eq!(ledger.claims[0].expires_at_secs, 155);

        // Another owner is refused and the claim is untouched.
        let other = ledger
            .refresh("do [#a]", "subagent:b", 106, 50)
            .unwrap_err();
        assert_eq!(
            other.reason,
            RefreshRefusal::OtherOwner {
                holder: "subagent:a".to_string()
            }
        );
        assert_eq!(ledger.claims[0].owner, "subagent:a");
        assert_eq!(ledger.claims[0].expires_at_secs, 155);

        // An expired claim is refused, never revived.
        let expired = ledger
            .refresh("do [#a]", "subagent:a", 155, 50)
            .unwrap_err();
        assert_eq!(expired.reason.kind(), "expired");
        assert_eq!(ledger.claims[0].expires_at_secs, 155);
        assert!(!ledger.claimed_items(155, None).claims("do [#a]"));

        // An unclaimed item is refused and no claim is created.
        let unclaimed = ledger
            .refresh("do [#b]", "subagent:a", 106, 50)
            .unwrap_err();
        assert_eq!(unclaimed.reason, RefreshRefusal::Unclaimed);
        assert_eq!(ledger.claims.len(), 1);
    }

    #[test]
    fn hash_id_resolves_a_preset_prefixed_head() {
        let heads = vec![
            "#gh-fix https://x/issues/110".to_string(),
            "#subagents do [#preflightdeadline]".to_string(),
        ];
        assert_eq!(
            resolve_claim_target("#preflightdeadline", &heads).unwrap(),
            "#subagents do [#preflightdeadline]"
        );
        assert_eq!(
            resolve_claim_target("#subagents do [#preflightdeadline]", &heads).unwrap(),
            "#subagents do [#preflightdeadline]"
        );
        let miss = resolve_claim_target("#nosuch", &heads).unwrap_err();
        let message = miss.to_string();
        assert!(
            message.starts_with("no live queue head matches \"#nosuch\"; live heads: "),
            "{message}"
        );
        assert!(
            message.contains("#subagents do [#preflightdeadline]"),
            "{message}"
        );
        // `#subagents do [#a]` now keys on `a` itself, so `#a` resolves it
        // directly; ambiguity needs two heads that only REFERENCE the id.
        assert_eq!(
            resolve_claim_target("#a", &["#subagents do [#a]".to_string()]).unwrap(),
            "#subagents do [#a]"
        );
        let twins = vec![
            "review do [#a] later".to_string(),
            "then also do [#a] again".to_string(),
        ];
        assert!(
            !resolve_claim_target("#a", &twins)
                .unwrap_err()
                .ambiguous
                .is_empty()
        );
        assert_eq!(
            referenced_queue_ids("#subagents do [#A] and do #b-2"),
            vec!["a", "b-2"]
        );
    }

    /// `#claimdispatchidentity` (live 2026-10-03): the operator edited the
    /// claimed `#subagents: <url>` into `#subagents: #gh-fix <url>` while the
    /// subagent worked. The claim must follow the edit (still claimed, still a
    /// live head, never pruned as closed), and a retarget to a different URL
    /// or id must not inherit it.
    #[test]
    fn claim_follows_a_tag_edit_but_not_a_retarget() {
        let url = "https://github.com/btakita/agent-doc/issues/120";
        let claimed_line = format!("#subagents: {url}");
        let edited_line = format!("#subagents: #gh-fix {url}");
        let mut ledger = QueueClaimLedger::default();
        ledger.claim(&claimed_line, "subagent:gh120", 100, 1000);

        let live_after_edit: HashSet<_> = [claim_identity(&edited_line)].into_iter().collect();
        let claimed = ledger.claimed_items(105, Some(&live_after_edit));
        assert!(
            claimed.claims(&edited_line),
            "the edited line keeps its claim"
        );
        assert!(claimed.claims(&format!("🚧 {edited_line}")));
        assert_eq!(
            ledger.prune(105, Some(&live_after_edit)),
            0,
            "an edited claimed line is not a closed item"
        );
        assert_eq!(
            resolve_claim_target(&claimed_line, std::slice::from_ref(&edited_line)).unwrap(),
            edited_line,
            "release/refresh by the original text still finds the edited head"
        );

        let retarget = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/121";
        let live_after_retarget: HashSet<_> = [claim_identity(retarget)].into_iter().collect();
        assert!(
            !ledger
                .claimed_items(105, Some(&live_after_retarget))
                .claims(retarget),
            "a different issue is a different task"
        );
        assert_eq!(ledger.prune(105, Some(&live_after_retarget)), 1);

        let mut ids = QueueClaimLedger::default();
        ids.claim("do [#a]", "subagent:a", 100, 1000);
        let items = ids.claimed_items(105, None);
        assert!(items.claims("#subagents do [#a]"));
        assert!(items.claims("#subagents: #gh-fix do [#a]"));
        assert!(
            !items.claims("#subagents do [#b]"),
            "a different id is a retarget"
        );
    }

    /// A ledger stored under the old tag-sensitive key is re-keyed on load, so
    /// a claim taken before the upgrade still covers the edited head.
    #[test]
    fn rekey_migrates_tag_sensitive_free_text_claims() {
        let url = "https://x/issues/120";
        let mut ledger = QueueClaimLedger {
            claims: vec![
                QueueClaim {
                    identity: QueueItemIdentity::FreeText(format!("#subagents: {url}")),
                    item_text: format!("#subagents: {url}"),
                    owner: "subagent:a".into(),
                    claimed_at_secs: 1,
                    expires_at_secs: 50,
                },
                QueueClaim {
                    identity: QueueItemIdentity::FreeText(format!("#gh-fix {url}")),
                    item_text: format!("#gh-fix {url}"),
                    owner: "subagent:b".into(),
                    claimed_at_secs: 2,
                    expires_at_secs: 90,
                },
            ],
        };
        assert!(ledger.rekey());
        assert_eq!(ledger.claims.len(), 1, "one work identity, one claim");
        assert_eq!(ledger.claims[0].owner, "subagent:b");
        assert!(
            ledger
                .claimed_items(10, None)
                .claims(&format!("#subagents: #gh-fix {url}"))
        );
        assert!(!ledger.rekey(), "rekey is idempotent");
    }

    /// GH #124 ask 5: a head present in the editor authority (what dispatch
    /// saw) but not yet saved to disk is still claimable.
    #[test]
    fn claim_target_resolves_in_the_editor_authority_view_on_a_disk_miss() {
        let disk = vec!["do [#restartideload]".to_string()];
        let authority = vec![
            "do [#restartideload]".to_string(),
            "#bug: The tmux panes are swapped.".to_string(),
        ];
        assert_eq!(
            resolve_claim_target_in_views(
                "#bug: The tmux panes are swapped.",
                &disk,
                Some(&authority)
            )
            .unwrap(),
            (
                "#bug: The tmux panes are swapped.".to_string(),
                ClaimHeadSource::EditorAuthority
            )
        );
        assert_eq!(
            resolve_claim_target_in_views("#restartideload", &disk, Some(&authority))
                .unwrap()
                .1,
            ClaimHeadSource::Disk
        );
        assert!(
            resolve_claim_target_in_views("#bug: The tmux panes are swapped.", &disk, None)
                .is_err()
        );
        let miss = resolve_claim_target_in_views("#nosuch", &disk, Some(&authority)).unwrap_err();
        assert!(
            miss.live_heads
                .contains(&"#bug: The tmux panes are swapped.".to_string()),
            "{miss}"
        );
    }

    #[test]
    fn expiry_and_closure_end_a_claim() {
        let mut ledger = QueueClaimLedger::default();
        ledger.claim("do [#a]", "subagent:a", 100, 10);
        ledger.claim("free text item", "subagent:b", 100, 1000);
        assert!(ledger.claimed_items(105, None).claims("do [#a]"));
        assert!(
            !ledger.claimed_items(110, None).claims("do [#a]"),
            "an expired claim must restore drainability"
        );
        let live: HashSet<_> = [claim_identity("do [#a]")].into_iter().collect();
        assert!(
            !ledger
                .claimed_items(105, Some(&live))
                .claims("free text item"),
            "a claim on an item no longer in the queue is closed"
        );
        assert_eq!(ledger.prune(110, Some(&live)), 2);
        assert!(ledger.claims.is_empty());
    }
}

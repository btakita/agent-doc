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
//! A claim keys on [`QueueItemIdentity::from_prompt`], the same identity queue
//! convergence and dedup use: `#id` for id-backed heads, marker-invariant
//! normalized free text otherwise (`strip_priority_markers`). So `🚧 do [#a]`,
//! `do [#a]` and `[#a]` are one claim, and a preset invocation such as
//! `#gh-fix <url>` keys on its full text.
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
//!   back to ordinary drainability. Re-claiming refreshes the TTL, which lets a
//!   long-running owner keep its claim alive.
//!
//! This module is pure: no clock, no storage. The caller supplies `now`.

use std::collections::HashSet;

use agent_doc_element_queue::QueueItemIdentity;
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

/// The identity a claim on `item` keys on. Strips the `🚧` in-progress marker
/// and other priority markers through [`QueueItemIdentity::from_prompt`].
pub fn claim_identity(item: &str) -> QueueItemIdentity {
    QueueItemIdentity::from_prompt(item.trim())
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
        let twins = vec![
            "#subagents do [#a]".to_string(),
            "review do [#a] later".to_string(),
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

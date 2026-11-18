//! Deterministic `#address` completion (`#addressproof`).
//!
//! `FIX-ON-HEAD` and `NO-WRITTEN-FIX` are the worker's claim. They are not
//! proof that the address changed the reviewed head or posted a review reply.
//! Completion reads an observation of the pull request. A worker hand-back is
//! never that observation. An independent validator's observation counts only
//! when the remote itself could not be read.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A GitHub pull request named by an `#address` item, plus the review thread
/// when the item names one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressTarget {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub discussion_id: Option<u64>,
    pub review_id: Option<u64>,
}

impl AddressTarget {
    /// Stable key for one target. Discussion and review ids stay in the key so
    /// two threads on the same pull request do not share a baseline.
    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}#d{}#r{}",
            self.owner,
            self.repo,
            self.number,
            self.discussion_id.unwrap_or(0),
            self.review_id.unwrap_or(0),
        )
    }

    /// The item names one review thread, so a reply on that thread is proof
    /// even when no earlier baseline was stored.
    pub fn thread_named(&self) -> bool {
        self.discussion_id.is_some() || self.review_id.is_some()
    }
}

/// One read of a pull request's head and the replies on its addressed thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressSnapshot {
    pub head_sha: String,
    pub reply_ids: Vec<u64>,
    /// Commit the addressed review comment is attached to, when the remote
    /// reported one. Used only when no earlier baseline exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_commit: Option<String>,
    /// `true` when the queue item named one discussion or review thread.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub thread_named: bool,
}

/// What the binary could read about an address target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressObservation {
    /// `baseline` is an earlier read, when one was stored before this check.
    Seen {
        baseline: Option<AddressSnapshot>,
        current: AddressSnapshot,
    },
    /// The remote could not be read. This is not a worker claim.
    Unseen,
}

/// The fact that lets an `#address` run complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressProof {
    /// The pull-request head SHA changed.
    HeadChanged { from: String, to: String },
    /// A review reply was posted on the addressed thread.
    ReviewReply { reply_id: u64 },
}

/// Whether an `#address` run may complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressCompletion {
    /// The observation shows a head change or a review reply.
    Proven(AddressProof),
    /// The head is unchanged and no review reply was posted.
    NoEffect { head: String },
    /// The remote was not read, and no independent validator supplied a read.
    Unseen,
}

/// Facts installed for the strike and report decisions on this thread.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddressEffectFacts {
    pub by_key: BTreeMap<String, AddressObservation>,
    /// Honored only for an [`AddressObservation::Unseen`] target.
    pub validator: Option<AddressObservation>,
}

thread_local! {
    static FACTS: RefCell<Option<AddressEffectFacts>> = const { RefCell::new(None) };
    static FORCED: RefCell<Option<AddressObservation>> = const { RefCell::new(None) };
}

/// Installs address observations until dropped. Nested installs restore the
/// previous facts.
#[derive(Debug)]
#[must_use]
pub struct AddressEffectGuard {
    previous: Option<AddressEffectFacts>,
}

impl AddressEffectGuard {
    pub fn install(facts: AddressEffectFacts) -> Self {
        let previous = FACTS.with(|slot| slot.borrow_mut().replace(facts));
        Self { previous }
    }

    /// One item's observation. An item that is not an address target is recorded
    /// under its own text so a missing parse cannot look proven.
    pub fn install_item(item: &str, observation: AddressObservation) -> Self {
        let mut by_key = BTreeMap::new();
        let key = address_review_target(item)
            .map(|target| target.key())
            .unwrap_or_else(|| item.to_string());
        by_key.insert(key, observation);
        Self::install(AddressEffectFacts {
            by_key,
            validator: None,
        })
    }
}

impl Drop for AddressEffectGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        FACTS.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// Forces the observation `report-complete` reads, until dropped.
///
/// Tests use this instead of the network. Production leaves it unset and reads
/// the remote.
#[derive(Debug)]
#[must_use]
pub struct ForcedObservation {
    previous: Option<AddressObservation>,
}

impl ForcedObservation {
    pub fn install(observation: AddressObservation) -> Self {
        let previous = FORCED.with(|slot| slot.borrow_mut().replace(observation));
        Self { previous }
    }
}

impl Drop for ForcedObservation {
    fn drop(&mut self) {
        let previous = self.previous.take();
        FORCED.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// The observation a test forced onto this thread, if any.
pub fn forced_observation() -> Option<AddressObservation> {
    FORCED.with(|slot| slot.borrow().clone())
}

/// The pull-request target of an `#address` item, when it names one.
pub fn address_review_target(item: &str) -> Option<AddressTarget> {
    if !crate::address_fix_on_head::item_mentions_address_preset(item) {
        return None;
    }
    let mut found = None;
    for target in pull_targets(item) {
        if target.thread_named() {
            return Some(target);
        }
        if found.is_none() {
            found = Some(target);
        }
    }
    found
}

/// True when `item` is an `#address` of a GitHub pull request and the installed
/// observation does not show a head change or a review reply.
///
/// No installed observation is not proof. An item that is not such an address
/// is not blocked here.
pub fn address_effect_unproven(item: &str) -> bool {
    let Some(target) = address_review_target(item) else {
        return false;
    };
    let Some(facts) = FACTS.with(|slot| slot.borrow().clone()) else {
        return true;
    };
    let Some(observation) = facts.by_key.get(&target.key()) else {
        return true;
    };
    let validator = match observation {
        AddressObservation::Unseen => facts.validator.as_ref(),
        AddressObservation::Seen { .. } => None,
    };
    !matches!(
        judge_address_completion(observation, validator),
        AddressCompletion::Proven(_)
    )
}

/// Decide completion from a remote read.
///
/// `independent_validator` is used only when `observation` is
/// [`AddressObservation::Unseen`]. A seen read that shows no head change and
/// no review reply stays [`AddressCompletion::NoEffect`] even if the validator
/// claims otherwise.
pub fn judge_address_completion(
    observation: &AddressObservation,
    independent_validator: Option<&AddressObservation>,
) -> AddressCompletion {
    match observation {
        AddressObservation::Seen { baseline, current } => judge_seen(baseline.as_ref(), current),
        AddressObservation::Unseen => match independent_validator {
            Some(AddressObservation::Seen { baseline, current }) => {
                judge_seen(baseline.as_ref(), current)
            }
            _ => AddressCompletion::Unseen,
        },
    }
}

fn judge_seen(baseline: Option<&AddressSnapshot>, current: &AddressSnapshot) -> AddressCompletion {
    if let Some(baseline) = baseline {
        if head_changed(&baseline.head_sha, &current.head_sha) {
            return AddressCompletion::Proven(AddressProof::HeadChanged {
                from: baseline.head_sha.clone(),
                to: current.head_sha.clone(),
            });
        }
        if let Some(reply_id) = new_reply(&baseline.reply_ids, &current.reply_ids) {
            return AddressCompletion::Proven(AddressProof::ReviewReply { reply_id });
        }
        return AddressCompletion::NoEffect {
            head: current.head_sha.clone(),
        };
    }
    if current.thread_named
        && let Some(reply_id) = current.reply_ids.iter().copied().min()
    {
        return AddressCompletion::Proven(AddressProof::ReviewReply { reply_id });
    }
    if current.thread_named
        && let Some(comment) = current.comment_commit.as_deref()
        && head_changed(comment, &current.head_sha)
    {
        return AddressCompletion::Proven(AddressProof::HeadChanged {
            from: comment.to_string(),
            to: current.head_sha.clone(),
        });
    }
    AddressCompletion::NoEffect {
        head: current.head_sha.clone(),
    }
}

fn new_reply(baseline: &[u64], current: &[u64]) -> Option<u64> {
    current
        .iter()
        .copied()
        .filter(|id| !baseline.contains(id))
        .min()
}

fn head_changed(from: &str, to: &str) -> bool {
    let from = from.trim();
    let to = to.trim();
    !from.is_empty() && !to.is_empty() && !same_sha(from, to)
}

fn same_sha(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn pull_targets(item: &str) -> Vec<AddressTarget> {
    const PREFIX: &str = "https://github.com/";
    let mut targets = Vec::new();
    let mut rest = item;
    while let Some(offset) = rest.find(PREFIX) {
        let after = &rest[offset + PREFIX.len()..];
        if let Some(target) = one_pull_target(after) {
            targets.push(target);
        }
        rest = &rest[offset + PREFIX.len()..];
    }
    targets
}

fn one_pull_target(rest: &str) -> Option<AddressTarget> {
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    let kind = parts.next()?;
    let number_and_tail = parts.next()?;
    if kind != "pull" || !github_slug(owner) || !github_slug(repo) {
        return None;
    }
    let digits = number_and_tail
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    let number: u64 = number_and_tail[..digits].parse().ok()?;
    let tail = &number_and_tail[digits..];
    if tail
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric())
    {
        return None;
    }
    let (discussion_id, review_id) = fragment_ids(tail);
    Some(AddressTarget {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
        discussion_id,
        review_id,
    })
}

fn fragment_ids(tail: &str) -> (Option<u64>, Option<u64>) {
    let discussion_id = fragment_number(tail, "discussion_r");
    let review_id = fragment_number(tail, "pullrequestreview-");
    (discussion_id, review_id)
}

fn fragment_number(tail: &str, marker: &str) -> Option<u64> {
    let start = tail.find(marker)? + marker.len();
    let digits = tail[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .count();
    if digits == 0 {
        return None;
    }
    tail[start..start + digits].parse().ok()
}

fn github_slug(part: &str) -> bool {
    let mut chars = part.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric())
        && part
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        && !part.ends_with('.')
        && !part.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEM: &str =
        "#address https://github.com/sample-org/sample-repo/pull/302#discussion_r4606896223";
    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NEXT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn snap(head: &str, replies: &[u64], comment: Option<&str>) -> AddressSnapshot {
        AddressSnapshot {
            head_sha: head.to_string(),
            reply_ids: replies.to_vec(),
            comment_commit: comment.map(str::to_string),
            thread_named: true,
        }
    }

    fn seen(baseline: Option<AddressSnapshot>, current: AddressSnapshot) -> AddressObservation {
        AddressObservation::Seen { baseline, current }
    }

    #[test]
    fn a_discussion_address_parses_its_thread() {
        let target = address_review_target(ITEM).expect("discussion address");
        assert_eq!(target.owner, "sample-org");
        assert_eq!(target.repo, "sample-repo");
        assert_eq!(target.number, 302);
        assert_eq!(target.discussion_id, Some(4_606_896_223));
        assert_eq!(target.review_id, None);
        assert!(target.thread_named());
        assert!(address_review_target("do [#other]").is_none());
        assert!(
            address_review_target(
                "#adversarial-pr https://github.com/sample-org/sample-repo/pull/302"
            )
            .is_none()
        );
    }

    #[test]
    fn no_commit_and_no_reply_does_not_complete() {
        let current = snap(HEAD, &[], Some(HEAD));
        let same = seen(Some(snap(HEAD, &[], Some(HEAD))), current.clone());
        assert_eq!(
            judge_address_completion(&same, None),
            AddressCompletion::NoEffect {
                head: HEAD.to_string()
            }
        );
        let first_sight = seen(None, current);
        assert_eq!(
            judge_address_completion(&first_sight, None),
            AddressCompletion::NoEffect {
                head: HEAD.to_string()
            }
        );
        let item = ITEM;
        assert!(address_effect_unproven(item));
        let _guard = AddressEffectGuard::install_item(item, same);
        assert!(
            address_effect_unproven(item),
            "a receipt-shaped claim is not installed here; the seen read has no effect"
        );
    }

    #[test]
    fn a_changed_head_completes() {
        let observation = seen(Some(snap(HEAD, &[], None)), snap(NEXT, &[], None));
        assert_eq!(
            judge_address_completion(&observation, None),
            AddressCompletion::Proven(AddressProof::HeadChanged {
                from: HEAD.to_string(),
                to: NEXT.to_string(),
            })
        );
        let _guard = AddressEffectGuard::install_item(ITEM, observation);
        assert!(!address_effect_unproven(ITEM));
    }

    #[test]
    fn a_posted_review_reply_completes() {
        let observation = seen(Some(snap(HEAD, &[], None)), snap(HEAD, &[99], None));
        assert_eq!(
            judge_address_completion(&observation, None),
            AddressCompletion::Proven(AddressProof::ReviewReply { reply_id: 99 })
        );
        let first_sight = seen(None, snap(HEAD, &[99], Some(HEAD)));
        assert_eq!(
            judge_address_completion(&first_sight, None),
            AddressCompletion::Proven(AddressProof::ReviewReply { reply_id: 99 })
        );
    }

    #[test]
    fn a_head_that_moved_past_the_review_comment_completes_without_a_baseline() {
        let observation = seen(None, snap(NEXT, &[], Some(HEAD)));
        assert_eq!(
            judge_address_completion(&observation, None),
            AddressCompletion::Proven(AddressProof::HeadChanged {
                from: HEAD.to_string(),
                to: NEXT.to_string(),
            })
        );
    }

    #[test]
    fn an_unseen_remote_does_not_complete_unless_a_validator_saw_the_effect() {
        assert_eq!(
            judge_address_completion(&AddressObservation::Unseen, None),
            AddressCompletion::Unseen
        );
        let lying = seen(Some(snap(HEAD, &[], None)), snap(NEXT, &[], None));
        let seen_idle = seen(Some(snap(HEAD, &[], None)), snap(HEAD, &[], Some(HEAD)));
        assert_eq!(
            judge_address_completion(&seen_idle, Some(&lying)),
            AddressCompletion::NoEffect {
                head: HEAD.to_string()
            },
            "a validator does not override a remote read that shows no effect"
        );
        assert_eq!(
            judge_address_completion(&AddressObservation::Unseen, Some(&lying)),
            AddressCompletion::Proven(AddressProof::HeadChanged {
                from: HEAD.to_string(),
                to: NEXT.to_string(),
            })
        );
        let idle_validator = seen(Some(snap(HEAD, &[], None)), snap(HEAD, &[], None));
        assert_eq!(
            judge_address_completion(&AddressObservation::Unseen, Some(&idle_validator)),
            AddressCompletion::NoEffect {
                head: HEAD.to_string()
            }
        );
    }

    #[test]
    fn a_pull_request_without_a_named_thread_needs_a_baseline_delta() {
        let mut current = snap(NEXT, &[7], Some(HEAD));
        current.thread_named = false;
        let first_sight = seen(None, current.clone());
        assert_eq!(
            judge_address_completion(&first_sight, None),
            AddressCompletion::NoEffect {
                head: NEXT.to_string()
            },
            "an existing reply or an old commit is not proof for a whole pull request"
        );
        let moved = seen(
            Some(snap(HEAD, &[], None)),
            AddressSnapshot {
                thread_named: false,
                ..current
            },
        );
        assert!(matches!(
            judge_address_completion(&moved, None),
            AddressCompletion::Proven(AddressProof::HeadChanged { .. })
        ));
    }
}

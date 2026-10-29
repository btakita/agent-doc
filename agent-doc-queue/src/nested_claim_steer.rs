//! Claim-ledger view of `#nestedclaimsteer`.
//!
//! The document parse stays folded. While a guard is installed, live-head and
//! dispatch readers expand a claimed row: the first post-claim direct child
//! stays in the parent, and each later direct child is its own entry. An
//! empty guard is the identity. Projection and render of the parsed entries
//! never go through this view.

use std::borrow::Cow;
use std::cell::RefCell;

use agent_doc_element_queue::nested_claim_steer::split_nested_claim_steer_with_limit;
use agent_doc_element_queue::subprompts::has_subprompts;

use crate::document_queue::{QueueEntry, QueuePrompt};
use crate::queue_claim::{QueueClaim, claim_identity};

thread_local! {
    static STEER_SNAPSHOTS: RefCell<Vec<ClaimTimeSnapshot>> = const { RefCell::new(Vec::new()) };
}

/// Claim-time text of one active claim, keyed by the claim's current identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimTimeSnapshot {
    pub identity: agent_doc_element_queue::QueueItemIdentity,
    pub claim_time_text: String,
    /// Number of post-claim direct additions that still steer this attempt.
    /// A reported worker is finished and therefore has no steering slot.
    pub steering_addition_limit: usize,
}

impl ClaimTimeSnapshot {
    /// Row text post-claim additions are measured against.
    ///
    /// `claim_time_row` is that row when the claim key is a nest vector
    /// (`#nestlistlineage`). Otherwise the oldest `followed_from` entry is the
    /// row as claimed, and `item_text` is the fallback.
    pub fn from_claim(claim: &QueueClaim) -> Self {
        Self {
            identity: claim.identity.clone(),
            claim_time_text: claim
                .claim_time_row
                .clone()
                .or_else(|| claim.followed_from.first().cloned())
                .unwrap_or_else(|| claim.item_text.clone()),
            steering_addition_limit: match claim.state {
                crate::queue_claim::QueueClaimState::Claimed
                | crate::queue_claim::QueueClaimState::Running => 1,
                crate::queue_claim::QueueClaimState::Reported
                | crate::queue_claim::QueueClaimState::Lost => 0,
            },
        }
    }
}

/// Installs claim-time snapshots for the current thread until dropped.
///
/// Nested installs restore the previous set. An empty install is a no-op
/// when used through [`NestedClaimSteerGuard::install_if_nonempty`], so a
/// caller without claims does not clear a guard it is nested inside.
#[derive(Debug)]
#[must_use]
pub struct NestedClaimSteerGuard {
    previous: Vec<ClaimTimeSnapshot>,
}

impl NestedClaimSteerGuard {
    pub fn install(snapshots: Vec<ClaimTimeSnapshot>) -> Self {
        let previous = STEER_SNAPSHOTS.with(|slot| {
            let mut borrowed = slot.borrow_mut();
            std::mem::replace(&mut *borrowed, snapshots)
        });
        Self { previous }
    }

    pub fn install_if_nonempty(snapshots: Vec<ClaimTimeSnapshot>) -> Option<Self> {
        if snapshots.is_empty() {
            None
        } else {
            Some(Self::install(snapshots))
        }
    }
}

impl Drop for NestedClaimSteerGuard {
    fn drop(&mut self) {
        let previous = std::mem::take(&mut self.previous);
        STEER_SNAPSHOTS.with(|slot| {
            *slot.borrow_mut() = previous;
        });
    }
}

fn installed_snapshots() -> Option<Vec<ClaimTimeSnapshot>> {
    STEER_SNAPSHOTS.with(|slot| {
        let borrowed = slot.borrow();
        if borrowed.is_empty() {
            None
        } else {
            Some(borrowed.clone())
        }
    })
}

/// Expand `entries` when a guard is installed. Otherwise return them as-is.
pub fn expand_if_installed(entries: Vec<QueueEntry>) -> Vec<QueueEntry> {
    match installed_snapshots() {
        Some(snapshots) => expand_entries(entries, &snapshots),
        None => entries,
    }
}

/// Borrow `entries` when no guard is installed, so the hot path does not clone.
pub fn expand_slice_if_installed(entries: &[QueueEntry]) -> Cow<'_, [QueueEntry]> {
    match installed_snapshots() {
        Some(snapshots) => Cow::Owned(expand_entries(entries.to_vec(), &snapshots)),
        None => Cow::Borrowed(entries),
    }
}

/// Split claimed prompts whose post-claim direct children route.
///
/// The steering prompt is not split again. Each routable child is expanded
/// on its own, so a claimed nested row applies the same rule one depth down.
pub fn expand_entries(
    entries: Vec<QueueEntry>,
    snapshots: &[ClaimTimeSnapshot],
) -> Vec<QueueEntry> {
    if snapshots.is_empty() {
        return entries;
    }
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            QueueEntry::Prompt(prompt) if !prompt.multiline => {
                expand_prompt(&prompt, snapshots, &mut out);
            }
            other => out.push(other),
        }
    }
    out
}

fn expand_prompt(prompt: &QueuePrompt, snapshots: &[ClaimTimeSnapshot], out: &mut Vec<QueueEntry>) {
    let index = out.len();
    out.push(QueueEntry::Prompt(prompt.clone()));
    // Parents already pushed supply the nest vector (`#nestlistlineage`).
    let identity = crate::queue_claim::claim_identity_at(out, index)
        .unwrap_or_else(|| claim_identity(&prompt.text));
    let Some(snapshot) = snapshots
        .iter()
        .find(|snapshot| snapshot.identity == identity)
    else {
        return;
    };
    if !has_subprompts(&prompt.text) {
        return;
    }
    let split = split_nested_claim_steer_with_limit(
        &prompt.text,
        &snapshot.claim_time_text,
        snapshot.steering_addition_limit,
    );
    if split.routable.is_empty() {
        return;
    }
    out.pop();
    let mut steering = prompt.clone();
    steering.text = split.steering.into_owned();
    steering.multiline = false;
    out.push(QueueEntry::Prompt(steering));
    for child in split.routable {
        let child_prompt = QueuePrompt {
            text: child.head_text,
            multiline: false,
            indent: child.indent,
            ordered_marker: child.ordered_marker,
        };
        expand_prompt(&child_prompt, snapshots, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_queue::{self, entry_is_group_parent};

    fn parent_snapshot(text: &str) -> ClaimTimeSnapshot {
        ClaimTimeSnapshot {
            identity: claim_identity(text),
            claim_time_text: text.to_string(),
            steering_addition_limit: 1,
        }
    }

    #[test]
    fn appended_siblings_render_back_to_the_same_body() {
        let body = "- Parent\n  - alpha\n  - beta\n";
        let entries = document_queue::parse(body).unwrap();
        let expanded = expand_entries(entries.clone(), &[parent_snapshot("Parent")]);
        assert_eq!(
            document_queue::render(&expanded),
            document_queue::render(&entries)
        );
        assert_eq!(document_queue::render(&expanded), body);
        let prompts = document_queue::prompts(&expanded);
        assert_eq!(prompts.len(), 2);
        assert!(has_subprompts(&prompts[0].text));
        assert!(!entry_is_group_parent(&expanded, 0));
        assert_eq!(prompts[1].text, "beta");
        assert_eq!(prompts[1].indent, 2);
        let nest = document_queue::nest_vector_for_prompt_text(&expanded, "beta").unwrap();
        assert_eq!(nest, "Parent: beta");
        assert!(
            document_queue::nest_vector_for_prompt_text(&entries, "Parent\n  - alpha\n  - beta")
                .is_none()
        );
    }

    #[test]
    fn an_ordered_routable_child_keeps_its_marker() {
        let body = "- Parent\n  1. alpha\n  2. beta\n";
        let entries = document_queue::parse(body).unwrap();
        let expanded = expand_entries(entries.clone(), &[parent_snapshot("Parent")]);
        assert_eq!(document_queue::render(&expanded), body);
        let prompts = document_queue::prompts(&expanded);
        assert_eq!(prompts[1].ordered_marker.as_deref(), Some("2."));
    }

    #[test]
    fn a_claimed_routable_child_splits_its_own_grandchildren() {
        let body = "- Parent\n  - alpha\n  - beta\n    - first gc\n    - second gc\n";
        let entries = document_queue::parse(body).unwrap();
        // `beta` is a nested leaf, so its claim key is the nest vector
        // (`#nestlistlineage`). Claim-time text stays the row, which is what
        // later grandchildren are measured against.
        let beta = ClaimTimeSnapshot {
            identity: claim_identity("Parent: beta"),
            claim_time_text: "beta".to_string(),
            steering_addition_limit: 1,
        };
        let snapshots = vec![parent_snapshot("Parent"), beta];
        let expanded = expand_entries(entries, &snapshots);
        let prompts = document_queue::prompts(&expanded);
        assert_eq!(
            prompts.len(),
            3,
            "{:?}",
            prompts.iter().map(|p| &p.text).collect::<Vec<_>>()
        );
        assert_eq!(prompts[0].text, "Parent\n  - alpha");
        assert_eq!(prompts[1].text, "beta\n    - first gc");
        assert_eq!(prompts[1].indent, 2);
        assert_eq!(prompts[2].text, "second gc");
        assert_eq!(prompts[2].indent, 4);
        let nest = document_queue::nest_vector_for_prompt_text(&expanded, "second gc").unwrap();
        assert_eq!(nest, "Parent: beta: second gc");
        assert!(!entry_is_group_parent(&expanded, 1));
    }

    #[test]
    fn an_unclaimed_fold_is_unchanged_without_snapshots() {
        let body = "- Parent\n  - alpha\n  - beta\n";
        let entries = document_queue::parse(body).unwrap();
        let expanded = expand_entries(entries.clone(), &[]);
        assert_eq!(expanded, entries);
        assert!(entry_is_group_parent(
            &document_queue::parse("- epic\n  - do [#alpha]\n  - remember the logs\n").unwrap(),
            0
        ));
    }

    #[test]
    fn a_lineage_claim_measures_children_against_the_stored_row() {
        let mut ledger = crate::queue_claim::QueueClaimLedger::default();
        ledger.claim("Parent: beta", "subagent:route", 1, 100);
        ledger.claims[0].claim_time_row = Some("beta".to_string());
        let snapshot = ClaimTimeSnapshot::from_claim(&ledger.claims[0]);
        assert_eq!(snapshot.claim_time_text, "beta");
        assert_eq!(snapshot.identity, claim_identity("Parent: beta"));
        assert_eq!(snapshot.steering_addition_limit, 1);
    }

    #[test]
    fn a_reported_claim_routes_the_first_post_claim_child() {
        let body = "- Parent\n  - follow-up\n";
        let entries = document_queue::parse(body).unwrap();
        let snapshot = ClaimTimeSnapshot {
            identity: claim_identity("Parent"),
            claim_time_text: "Parent".to_string(),
            steering_addition_limit: 0,
        };
        let expanded = expand_entries(entries, &[snapshot]);
        let prompts = document_queue::prompts(&expanded);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].text, "follow-up");
        assert_eq!(prompts[0].indent, 2);
        assert_eq!(
            document_queue::nest_vector_for_prompt_text(&expanded, "follow-up").as_deref(),
            Some("Parent: follow-up")
        );
    }

    #[test]
    fn the_guard_restores_the_previous_snapshots() {
        let outer = NestedClaimSteerGuard::install(vec![parent_snapshot("outer")]);
        let inner = NestedClaimSteerGuard::install(vec![parent_snapshot("inner")]);
        drop(inner);
        let restored = installed_snapshots().unwrap();
        assert_eq!(restored[0].claim_time_text, "outer");
        drop(outer);
        assert!(installed_snapshots().is_none());
    }
}

//! Claim-ledger view of `#nestedclaimsteer`, and the checked-decision release
//! (`#checkeddecisionsubprompt`).
//!
//! The document parse stays folded. Drain and dispatch readers always release
//! the live direct children of a checked decision parent, including the first
//! child and including when no claim snapshot is installed. While a guard is
//! installed, those readers also expand a claimed row: the first post-claim
//! direct child stays in the parent, and each later direct child is its own
//! entry. An empty guard leaves every other row folded. Projection and render
//! of the parsed entries never go through this view.

use std::borrow::Cow;
use std::cell::RefCell;

use agent_doc_element_queue::nested_claim_steer::{
    release_checked_decision_subprompts, split_nested_claim_steer,
};
use agent_doc_element_queue::subprompts::{self, has_subprompts};

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

/// Expand `entries` for drain and dispatch readers.
///
/// Checked-decision subprompts are released even when no guard is installed
/// (`#checkeddecisionsubprompt`). Claim-steer runs only while a guard holds
/// snapshots.
pub fn expand_if_installed(entries: Vec<QueueEntry>) -> Vec<QueueEntry> {
    let entries = release_checked_decision_entries(entries);
    match installed_snapshots() {
        Some(snapshots) => expand_entries(entries, &snapshots),
        None => entries,
    }
}

/// Borrow `entries` when neither a guard nor a checked-decision release
/// applies, so the hot path does not clone.
pub fn expand_slice_if_installed(entries: &[QueueEntry]) -> Cow<'_, [QueueEntry]> {
    let released = if slice_releases_checked_decision(entries) {
        Some(release_checked_decision_entries(entries.to_vec()))
    } else {
        None
    };
    match installed_snapshots() {
        Some(snapshots) => {
            let base = released.unwrap_or_else(|| entries.to_vec());
            Cow::Owned(expand_entries(base, &snapshots))
        }
        None => match released {
            Some(released) => Cow::Owned(released),
            None => Cow::Borrowed(entries),
        },
    }
}

fn slice_releases_checked_decision(entries: &[QueueEntry]) -> bool {
    entries.iter().any(|entry| {
        matches!(
            entry,
            QueueEntry::Prompt(prompt)
                if !prompt.multiline && subprompts::prompt_releases_live_subprompts(&prompt.text)
        )
    })
}

fn release_checked_decision_entries(entries: Vec<QueueEntry>) -> Vec<QueueEntry> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            QueueEntry::Prompt(prompt) if !prompt.multiline => release_prompt(&prompt, &mut out),
            other => out.push(other),
        }
    }
    out
}

fn release_prompt(prompt: &QueuePrompt, out: &mut Vec<QueueEntry>) {
    let Some((parent_text, released)) = release_checked_decision_subprompts(&prompt.text) else {
        out.push(QueueEntry::Prompt(prompt.clone()));
        return;
    };
    let (head, block) = subprompts::split_subprompt_block(&parent_text);
    let closed = subprompts::parent_line_is_struck(head)
        && block.is_none_or(|block| !subprompts::block_has_live_subprompt(block));
    let mut parent = prompt.clone();
    parent.text = parent_text;
    parent.multiline = false;
    if closed {
        out.push(QueueEntry::Completed(parent));
    } else {
        out.push(QueueEntry::Prompt(parent));
    }
    for child in released {
        release_prompt(
            &QueuePrompt {
                text: child.head_text,
                multiline: false,
                indent: child.indent,
                ordered_marker: child.ordered_marker,
            },
            out,
        );
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
    let split = split_nested_claim_steer(&prompt.text, &snapshot.claim_time_text);
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
    }

    /// `#checkeddecisionsubprompt`: no claim snapshot is required. The live
    /// subprompt, including the first one, is its own head. The checked parent
    /// stays a head. `🙋☑️` and a reported row with no decision id stay folded.
    #[test]
    fn a_checked_decision_subprompt_is_its_own_head_without_a_claim_snapshot() {
        let parent =
            "☑️ [#subagent:fix-ci#2] publish the contracts [#subagent-operator-request-contracts]";
        let body =
            format!("- {parent}\n  - ~~already answered~~\n  - Commit and fast-forward push\n");
        let doc = format!("<!-- agent:run subagents -->\n{body}<!-- /agent:run -->\n");
        let entries = document_queue::parse(&body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(document_queue::render(&entries), body);
        let expanded = expand_if_installed(entries);
        let prompts = document_queue::prompts(&expanded);
        assert_eq!(
            prompts.len(),
            2,
            "{:?}",
            prompts.iter().map(|p| &p.text).collect::<Vec<_>>()
        );
        assert!(prompts[0].text.contains("publish the contracts"));
        assert!(prompts[0].text.contains("~~already answered~~"));
        assert!(!prompts[0].text.contains("Commit and fast-forward push"));
        assert_eq!(prompts[1].text, "Commit and fast-forward push");
        assert_eq!(prompts[1].indent, 2);
        assert!(!document_queue::entry_is_group_parent(&expanded, 0));
        let heads = crate::queue_continuation::live_queue_head_texts(&doc).unwrap();
        assert!(
            heads
                .iter()
                .any(|head| head.contains("Commit and fast-forward push")),
            "{heads:?}"
        );
        assert!(
            heads
                .iter()
                .any(|head| head.contains("publish the contracts")
                    && !head.contains("Commit and fast-forward push")),
            "{heads:?}"
        );
        let child = heads
            .iter()
            .find(|head| head.contains("Commit and fast-forward push"))
            .unwrap();
        let parent_head = heads
            .iter()
            .find(|head| {
                head.contains("publish the contracts")
                    && !head.contains("Commit and fast-forward push")
            })
            .unwrap();
        assert_ne!(
            crate::queue_claim::claim_identity(parent_head),
            crate::queue_claim::claim_identity(child)
        );

        let held =
            "- 🙋☑️ publish the contracts [#subagent-operator-request-contracts]\n  - Commit and fast-forward push\n";
        let held_entries = document_queue::parse(held).unwrap();
        assert_eq!(expand_if_installed(held_entries.clone()), held_entries);
        let plain =
            "- ☑️ [#subagent:fix#2] publish the contracts\n  - Commit and fast-forward push\n";
        let plain_entries = document_queue::parse(plain).unwrap();
        assert_eq!(plain_entries.len(), 1);
        assert_eq!(expand_if_installed(plain_entries.clone()), plain_entries);

        let struck =
            "- ~~publish the contracts [#subagent-operator-request-contracts]~~\n  - Commit and fast-forward push\n";
        let struck_entries = document_queue::parse(struck).unwrap();
        assert!(matches!(
            struck_entries[0],
            document_queue::QueueEntry::Prompt(_)
        ));
        let struck_expanded = expand_if_installed(struck_entries);
        let struck_prompts = document_queue::prompts(&struck_expanded);
        assert_eq!(struck_prompts.len(), 1, "{struck_prompts:?}");
        assert_eq!(struck_prompts[0].text, "Commit and fast-forward push");
        assert!(matches!(
            struck_expanded[0],
            document_queue::QueueEntry::Completed(_)
        ));
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

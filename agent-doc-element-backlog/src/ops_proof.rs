//! Pure ops-proof completion classification for tracked-work items.
//!
//! This module is I/O-free. Callers own snapshots, cycle-state, ops-log writes,
//! and document mutation.

use std::collections::HashSet;

use agent_doc_element::element;

use crate::backlog::{self, PendingItem, PendingState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsProofCompletion {
    pub id: String,
    pub evidence: String,
}

/// Pending item ids present in `surface` within `content`.
///
/// This lets effectful callers detect brand-new same-cycle adds that were
/// absent from a cycle-start snapshot before accepting ops-proof auto-complete.
pub fn surface_pending_ids(content: &str, surface: &str) -> HashSet<String> {
    element::parse(content)
        .ok()
        .and_then(|comps| {
            comps
                .into_iter()
                .find(|c| backlog::component_matches_tracked_surface(&c.name, surface))
        })
        .map(|comp| {
            let (_, items, _) = backlog::parse_items(comp.content(content));
            items
                .into_iter()
                .map(|item| item.id)
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn ops_proof_completion_candidates(body: &str) -> Vec<OpsProofCompletion> {
    let (_, items, _) = backlog::parse_items(body);
    items
        .iter()
        .filter(|item| !matches!(item.state, PendingState::Done))
        .filter_map(|item| {
            classify_ops_proof_completion(item).map(|evidence| OpsProofCompletion {
                id: item.id.clone(),
                evidence,
            })
        })
        .collect()
}

pub fn classify_ops_proof_completion(item: &PendingItem) -> Option<String> {
    if item.id.is_empty() {
        return None;
    }
    let text = format!("{} {}", item.text, item.continuation);
    let upper = text.to_ascii_uppercase();
    if !has_ops_completion_marker(&upper) || has_ops_completion_blocker(&upper) {
        return None;
    }

    // #opsproofgate: a live-verify / operator-drive gate must NEVER be
    // auto-completed on `evidence=commit`. A shipped commit is not proof for
    // these items — only an anchored ops.log marker driven live by the operator
    // is. The log-arbiter path closes them on a genuine structured emission;
    // this commit/CI prose scan must stay out of its way.
    if is_live_verify_gate(&upper) {
        return None;
    }

    // #opsproof-falsepos: an open actionable item must NOT be reaped just
    // because its prose cites already-landed dependency work. The completion
    // marker must be the item's own leading status verb. Gated items were
    // deliberately code-completed by the agent, so a proven marker anywhere in
    // their text legitimately closes them.
    let is_gated = matches!(item.state, PendingState::Gated);
    if !is_gated && !marker_is_leading_status(&upper) {
        return None;
    }

    let has_commit = contains_commit_hash(&text);
    let has_ci = contains_successful_ci_proof(&upper);
    if !has_commit && !has_ci {
        return None;
    }

    Some(
        match (has_commit, has_ci) {
            (true, true) => "commit+ci",
            (true, false) => "commit",
            (false, true) => "ci",
            (false, false) => unreachable!(),
        }
        .to_string(),
    )
}

pub fn has_ops_completion_marker(upper: &str) -> bool {
    ["DONE", "SHIPPED", "IMPLEMENTED", "COMPLETE", "COMPLETED"]
        .iter()
        .any(|marker| contains_ascii_word(upper, marker))
}

/// Max number of leading words (after skipping `#hashtag` tokens) that count as
/// the item's status prefix for ops-proof auto-completion.
pub const LEADING_STATUS_WORDS: usize = 4;

/// True when an ops-completion marker is the item's leading status verb rather
/// than a marker buried in a cited dependency clause. The leading status segment
/// is the prefix before the first clause break (`: ` or `. `), further capped to
/// the first [`LEADING_STATUS_WORDS`] words after skipping leading `#hashtag`
/// tokens. `upper` must already be ASCII-uppercased.
pub fn marker_is_leading_status(upper: &str) -> bool {
    has_ops_completion_marker(&leading_status_segment(upper))
}

pub fn leading_status_segment(upper: &str) -> String {
    let mut cut = upper.len();
    for sep in [": ", ". "] {
        if let Some(idx) = upper.find(sep) {
            cut = cut.min(idx);
        }
    }
    upper[..cut]
        .split_whitespace()
        .filter(|word| !word.starts_with('#'))
        .take(LEADING_STATUS_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn has_ops_completion_blocker(upper: &str) -> bool {
    const BLOCKER_PHRASES: &[&str] = &[
        "COULD NOT",
        "CAN NOT",
        "CANNOT",
        "CAN'T",
        "FALSE CLOSEOUT",
        "FOLLOW-UP",
        "FOLLOW UP",
        "FOLLOWUPS",
        "NOT DONE",
        "NOT SHIPPED",
        "NOT IMPLEMENTED",
        "SUB-PART",
        "SUBPART",
    ];
    const BLOCKER_WORDS: &[&str] = &[
        "PARTIAL",
        "REMAINING",
        "REOPENED",
        "DEFERRED",
        "BLOCKED",
        "BLOCKER",
        "TODO",
        "WIP",
        "PARTLY",
        "FAILING",
        "FAILED",
    ];

    BLOCKER_PHRASES.iter().any(|phrase| upper.contains(phrase))
        || BLOCKER_WORDS
            .iter()
            .any(|word| contains_ascii_word(upper, word))
}

/// True when an item is a live-verify / operator-drive gate whose only valid
/// completion proof is an anchored structured ops.log marker driven live by the
/// operator — never a cited commit/CI reference. `upper` must already be
/// ASCII-uppercased.
pub fn is_live_verify_gate(upper: &str) -> bool {
    const LIVE_VERIFY_PHRASES: &[&str] = &[
        "LIVE-VERIFY GATE",
        "LIVE-VERIFY ONLY",
        "LIVE VERIFY GATE",
        "LIVE VERIFY ONLY",
        "OPERATOR-DRIVE",
        "OPERATOR DRIVE",
        "OPERATOR DRIVES",
        "OPERATOR LIVE-VERIFY",
        "OPERATOR LIVE VERIFY",
    ];
    LIVE_VERIFY_PHRASES
        .iter()
        .any(|phrase| upper.contains(phrase))
}

pub fn contains_successful_ci_proof(upper: &str) -> bool {
    contains_ascii_word(upper, "CI")
        && ["GREEN", "PASSED", "PASSING", "SUCCESS", "SUCCEEDED"]
            .iter()
            .any(|word| contains_ascii_word(upper, word))
}

pub fn contains_commit_hash(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| {
            (7..=40).contains(&token.len())
                && token.chars().all(|c| c.is_ascii_hexdigit())
                && token.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F'))
        })
}

pub fn contains_ascii_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(idx, _)| {
        let before = idx
            .checked_sub(1)
            .and_then(|pos| haystack.as_bytes().get(pos).copied());
        let after = haystack.as_bytes().get(idx + needle.len()).copied();
        before.is_none_or(|b| !b.is_ascii_alphanumeric())
            && after.is_none_or(|b| !b.is_ascii_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, state: PendingState, text: &str) -> PendingItem {
        PendingItem {
            marker: backlog::PendingListMarker::Bullet,
            id: id.to_string(),
            state,
            gate_type: None,
            in_progress: false,
            text: text.to_string(),
            continuation: String::new(),
        }
    }

    #[test]
    fn classifies_leading_status_with_commit_and_ci() {
        let result = classify_ops_proof_completion(&item(
            "doneci",
            PendingState::Open,
            "#agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): fixed it",
        ));
        assert_eq!(result.as_deref(), Some("commit+ci"));
    }

    #[test]
    fn rejects_blocked_partial_and_missing_proof_items() {
        assert_eq!(
            classify_ops_proof_completion(&item(
                "partial",
                PendingState::Open,
                "PARTIAL SHIPPED 9df1244f: REMAINING live proof gate",
            )),
            None
        );
        assert_eq!(
            classify_ops_proof_completion(&item(
                "noproof",
                PendingState::Open,
                "DONE: lacks deterministic proof",
            )),
            None
        );
    }

    #[test]
    fn rejects_cited_dependency_marker_for_open_items() {
        let result = classify_ops_proof_completion(&item(
            "citeddep",
            PendingState::Open,
            "wire the predicate into dispatch. The predicate already shipped in 600797b3",
        ));
        assert_eq!(result, None);
    }

    #[test]
    fn accepts_gated_completion_marker_anywhere() {
        let result = classify_ops_proof_completion(&item(
            "reviewdone",
            PendingState::Gated,
            "review-gated path SHIPPED abcdef1 (CI 2 passed)",
        ));
        assert_eq!(result.as_deref(), Some("commit+ci"));
    }

    #[test]
    fn rejects_live_verify_gate_on_commit_hash() {
        let result = classify_ops_proof_completion(&item(
            "ktw8",
            PendingState::Gated,
            "[live-verify gate] destructive path. Code SHIPPED 1edb20d2; operator drives proof.",
        ));
        assert_eq!(result, None);
    }

    #[test]
    fn surface_pending_ids_reads_backlog_aliases_and_review_surface() {
        let content = concat!(
            "<!-- agent:pending -->\n",
            "- [ ] [#a] one\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#r] review\n",
            "<!-- /agent:review -->\n",
        );
        assert_eq!(
            surface_pending_ids(content, "backlog"),
            HashSet::from(["a".to_string()])
        );
        assert_eq!(
            surface_pending_ids(content, "review"),
            HashSet::from(["r".to_string()])
        );
    }
}

//! Pure queue-head closeout guard policy.
//!
//! This module owns id-backed queue-head decisions used by session closeout
//! guards. Callers provide document text, cycle-state facts, and tracked-work
//! id sets; file IO, cycle-state loading, ops logs, and guard-mode formatting
//! stay in orchestration.

use std::collections::HashSet;

use agent_doc_document::queue_projection::strip_priority_markers;
use agent_doc_element::element;
use agent_doc_element_backlog::backlog;

use crate::{document_queue, queue_continuation, queue_directive, queue_heads, queue_response};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueHeadRemovalProofSource {
    BacklogResolvedOrRemoved,
    CycleLifecycleOutcome,
    CurrentDirectiveTarget,
}

impl QueueHeadRemovalProofSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BacklogResolvedOrRemoved => "backlog_resolved_or_removed",
            Self::CycleLifecycleOutcome => "cycle_lifecycle_outcome",
            Self::CurrentDirectiveTarget => "current_directive_target",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueHeadRemovalProof {
    pub id: String,
    pub source: QueueHeadRemovalProofSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueHeadRemovalDecision {
    pub lost: Vec<String>,
    pub removal_proofs: Vec<QueueHeadRemovalProof>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeTextQueueHeadProvenanceDecision {
    pub suppressed: bool,
    pub bare_heading_residue: bool,
    pub unresolved: Vec<String>,
    pub response_proven_removed: Vec<String>,
    pub completed_residue: Vec<String>,
}

/// `do [#id]` target ids present in a committed document's `agent:queue`
/// component.
pub fn committed_queue_head_ids(content: &str) -> Vec<String> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Vec::new();
    };
    queue_directive::do_directive_target_ids(&[queue.content(content).to_string()])
}

/// `do [#id]` target ids for the current live queue head only.
pub fn committed_current_queue_head_ids(content: &str) -> Vec<String> {
    let Ok(components) = element::parse(content) else {
        return Vec::new();
    };
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Vec::new();
    };
    let entries = document_queue::parse(queue.content(content)).unwrap_or_default();
    let Some(head) = document_queue::first_prompt(&entries) else {
        return Vec::new();
    };
    queue_directive::do_directive_target_ids(std::slice::from_ref(&head.text))
}

/// Recorded id-backed queue heads that still represent live unresponded work.
pub fn no_response_live_queue_head_ids(
    active_queue_heads: &[String],
    content: &str,
    open_backlog_ids: &HashSet<String>,
    resolved_ids: &HashSet<String>,
) -> Vec<String> {
    let recorded_ids = queue_directive::do_directive_target_ids(active_queue_heads);
    if recorded_ids.is_empty() {
        return Vec::new();
    }

    let current_head_ids: HashSet<String> = committed_current_queue_head_ids(content)
        .into_iter()
        .map(|id| normalize_id(&id))
        .collect();
    let deferred: HashSet<String> = queue_continuation::deferred_backlog_ids(content)
        .into_iter()
        .map(|id| normalize_id(&id))
        .collect();
    // `#opverifyanswered`: once the operator answers an `[operator-verify]` head
    // inline, it is live unresponded work again rather than a deferred head.
    let operator_answered: HashSet<String> = queue_continuation::operator_answered_head_ids(content)
        .into_iter()
        .map(|id| normalize_id(&id))
        .collect();

    let mut live = Vec::new();
    for id in recorded_ids {
        let norm = normalize_id(&id);
        if norm.is_empty() {
            continue;
        }
        if !current_head_ids.contains(&norm) || !open_backlog_ids.contains(&norm) {
            continue;
        }
        if resolved_ids.contains(&norm)
            || (deferred.contains(&norm) && !operator_answered.contains(&norm))
        {
            continue;
        }
        if !live.iter().any(|existing| existing == &norm) {
            live.push(norm);
        }
    }
    live
}

/// Reaped `do [#id]` directive heads, normalized and deterministically ordered.
pub fn reaped_queue_directive_head_ids(
    active_queue_heads: &[String],
    reaped_pending_ids: &[String],
) -> Vec<String> {
    let directive_ids: HashSet<String> =
        queue_directive::do_directive_target_ids(active_queue_heads)
            .into_iter()
            .map(|id| normalize_id(&id))
            .filter(|id| !id.is_empty())
            .collect();
    if directive_ids.is_empty() {
        return Vec::new();
    }

    let reaped: HashSet<String> = reaped_pending_ids
        .iter()
        .map(|id| normalize_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    if reaped.is_empty() {
        return Vec::new();
    }

    let mut ordered_ids: Vec<String> = directive_ids
        .into_iter()
        .filter(|id| reaped.contains(id))
        .collect();
    ordered_ids.sort();
    ordered_ids
}

/// Decide whether recorded queue heads were legitimately removed or silently
/// lost while their tracked-work items remained open.
pub fn queue_head_removal_decision(
    active_queue_heads: &[String],
    content: &str,
    open_backlog_ids: &HashSet<String>,
    resolved_ids: &HashSet<String>,
    directive_targets: &HashSet<String>,
) -> QueueHeadRemovalDecision {
    let recorded_ids = queue_directive::do_directive_target_ids(active_queue_heads);
    let still_queued: HashSet<String> = committed_queue_head_ids(content)
        .into_iter()
        .map(|id| normalize_id(&id))
        .collect();

    let mut lost = Vec::new();
    let mut removal_proofs = Vec::new();
    for id in recorded_ids {
        let norm = normalize_id(&id);
        if norm.is_empty() {
            continue;
        }
        if still_queued.contains(&norm) {
            continue;
        }
        if !open_backlog_ids.contains(&norm) {
            push_proof(
                &mut removal_proofs,
                norm,
                QueueHeadRemovalProofSource::BacklogResolvedOrRemoved,
            );
            continue;
        }
        if resolved_ids.contains(&norm) {
            push_proof(
                &mut removal_proofs,
                norm,
                QueueHeadRemovalProofSource::CycleLifecycleOutcome,
            );
            continue;
        }
        if directive_targets.contains(&norm) {
            push_proof(
                &mut removal_proofs,
                norm,
                QueueHeadRemovalProofSource::CurrentDirectiveTarget,
            );
            continue;
        }
        if !lost.iter().any(|existing| existing == &norm) {
            lost.push(norm);
        }
    }

    QueueHeadRemovalDecision {
        lost,
        removal_proofs,
    }
}

/// Classify recorded free-text queue heads for closeout provenance.
pub fn free_text_queue_head_provenance_decision(
    active_free_text_queue_heads: &[String],
    content: &str,
) -> Option<FreeTextQueueHeadProvenanceDecision> {
    if content.contains("<!-- no-free-text-queue-head-guard -->") {
        return Some(FreeTextQueueHeadProvenanceDecision {
            suppressed: true,
            bare_heading_residue: free_text_queue_marker_has_bare_heading_residue(content),
            unresolved: Vec::new(),
            response_proven_removed: Vec::new(),
            completed_residue: Vec::new(),
        });
    }

    let Ok(components) = element::parse(content) else {
        return None;
    };
    let exchange_text: String = components
        .iter()
        .find(|component| component.name == "exchange")
        .map(|component| component.content(content).to_string())
        .unwrap_or_default();
    let mut unresolved = Vec::new();
    let mut response_proven_removed = Vec::new();
    let mut completed_residue = Vec::new();

    for head in active_free_text_queue_heads {
        let head = strip_priority_markers(head);
        let normalized = head.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if queue_heads::is_do_directive(&head)
            || !queue_response::queue_prompt_text_is_free_text(content, &head)
        {
            continue;
        }
        if queue_heads::free_text_queue_head_is_completed_residue(content, &exchange_text, &head) {
            completed_residue.push(head);
            continue;
        }
        if queue_continuation::is_recurring_imperative_head(&head) {
            continue;
        }
        if queue_heads::committed_queue_contains_free_text_head(content, &head) {
            continue;
        }
        if queue_response::free_text_head_answered_by_response(&exchange_text, &head)
            || response_head_plausibly_answers(&exchange_text, &head)
        {
            response_proven_removed.push(head);
            continue;
        }
        unresolved.push(head);
    }

    Some(FreeTextQueueHeadProvenanceDecision {
        suppressed: false,
        bare_heading_residue: false,
        unresolved,
        response_proven_removed,
        completed_residue,
    })
}

fn normalize_id(id: &str) -> String {
    backlog::normalize_pending_id(id)
}

fn free_text_queue_marker_has_bare_heading_residue(content: &str) -> bool {
    content.contains("<!-- no-free-text-queue-head-guard -->")
        && content.lines().any(|line| line.trim() == "###")
}

fn response_head_plausibly_answers(content: &str, head: &str) -> bool {
    let head_words: Vec<&str> = head
        .split_whitespace()
        .filter(|w| {
            w.len() > 3
                && !matches!(
                    w.to_ascii_lowercase().as_str(),
                    "the"
                        | "this"
                        | "that"
                        | "with"
                        | "from"
                        | "also"
                        | "does"
                        | "what"
                        | "when"
                        | "how"
                )
        })
        .collect();
    if head_words.is_empty() {
        return false;
    }
    let lower = content.to_ascii_lowercase();
    let mut matched = 0;
    for word in &head_words {
        if lower.contains(&word.to_ascii_lowercase()) {
            matched += 1;
        }
    }
    matched * 2 >= head_words.len()
}

fn push_proof(
    proofs: &mut Vec<QueueHeadRemovalProof>,
    id: String,
    source: QueueHeadRemovalProofSource,
) {
    if !proofs.iter().any(|proof| proof.id == id) {
        proofs.push(QueueHeadRemovalProof { id, source });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(queue: &str, backlog: &str) -> String {
        format!(
            "<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n"
        )
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn committed_queue_head_ids_reads_all_ids_but_current_reads_head_only() {
        let content = doc("- do [#head]\n- do [#tail]\n", "");

        assert_eq!(
            committed_queue_head_ids(&content),
            vec!["head".to_string(), "tail".to_string()]
        );
        assert_eq!(
            committed_current_queue_head_ids(&content),
            vec!["head".to_string()]
        );
    }

    #[test]
    fn no_response_live_queue_head_ids_requires_current_open_unresolved_head() {
        let content = doc(
            "- do [#current]\n- do [#later]\n",
            "- [ ] [#current] current\n- [ ] [#later] later\n",
        );
        let live = no_response_live_queue_head_ids(
            &["do [#current]".to_string(), "do [#later]".to_string()],
            &content,
            &set(&["current", "later"]),
            &set(&["later"]),
        );

        assert_eq!(live, vec!["current".to_string()]);
    }

    #[test]
    fn no_response_live_queue_head_ids_skips_deferred_operator_verify_head() {
        let content = doc(
            "- do [#verify]\n",
            "- [ ] [#verify] [operator-verify] needs human review\n",
        );
        let live = no_response_live_queue_head_ids(
            &["do [#verify]".to_string()],
            &content,
            &set(&["verify"]),
            &HashSet::new(),
        );

        assert!(live.is_empty());
    }

    #[test]
    fn reaped_queue_directive_head_ids_normalizes_dedupes_and_sorts_intersection() {
        let ids = reaped_queue_directive_head_ids(
            &[
                "do [#Beta]".to_string(),
                "do [#alpha]".to_string(),
                "plain text".to_string(),
                "do [#beta]".to_string(),
                "do [#missing]".to_string(),
            ],
            &[
                "#beta".to_string(),
                "ALPHA".to_string(),
                "not-a-directive".to_string(),
            ],
        );

        assert_eq!(ids, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn queue_head_removal_decision_separates_lost_ids_from_proven_removals() {
        let content = doc(
            "- do [#kept]\n",
            "- [ ] [#kept] still queued\n- [ ] [#lost] open\n",
        );
        let decision = queue_head_removal_decision(
            &[
                "do [#kept]".to_string(),
                "do [#done]".to_string(),
                "do [#target]".to_string(),
                "do [#lost]".to_string(),
            ],
            &content,
            &set(&["kept", "target", "lost"]),
            &set(&["done"]),
            &set(&["target"]),
        );

        assert_eq!(decision.lost, vec!["lost".to_string()]);
        assert_eq!(
            decision.removal_proofs,
            vec![
                QueueHeadRemovalProof {
                    id: "done".to_string(),
                    source: QueueHeadRemovalProofSource::BacklogResolvedOrRemoved,
                },
                QueueHeadRemovalProof {
                    id: "target".to_string(),
                    source: QueueHeadRemovalProofSource::CurrentDirectiveTarget,
                },
            ]
        );
    }

    #[test]
    fn free_text_queue_head_provenance_decision_classifies_each_outcome() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- still queued\n",
            "- answered but still queued\n",
            "- deploy\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: answered but still queued\n\n",
            "> **Queue prompt:**\n>\n> answered but still queued\n\n",
            "Answered.\n\n",
            "### Re: removed with echo\n\n",
            "> **Queue prompt:**\n>\n> removed with echo\n\n",
            "Answered.\n\n",
            "### Re: removed by heading\n\n",
            "Removed by heading.\n\n",
            "### Re: deploy\n\n",
            "> **Queue prompt:**\n>\n> deploy\n\n",
            "Deployment completed.\n",
            "<!-- /agent:exchange -->\n",
        );
        let heads = vec![
            "still queued".to_string(),
            "answered but still queued".to_string(),
            "removed with echo".to_string(),
            "removed by heading".to_string(),
            "missing response".to_string(),
            "deploy".to_string(),
        ];

        let decision = free_text_queue_head_provenance_decision(&heads, content).unwrap();

        assert!(!decision.suppressed);
        assert!(!decision.bare_heading_residue);
        assert_eq!(
            decision.completed_residue,
            vec!["answered but still queued".to_string()]
        );
        assert_eq!(
            decision.response_proven_removed,
            vec![
                "removed with echo".to_string(),
                "removed by heading".to_string()
            ]
        );
        assert_eq!(decision.unresolved, vec!["missing response".to_string()]);
    }

    #[test]
    fn free_text_queue_head_provenance_decision_ignores_decorated_id_backed_state() {
        let content = doc("- [#sy71]\n", "");
        let heads = vec![
            "🚧 [#sy71]".to_string(),
            "🚧 do [#build]".to_string(),
            "🚧 missing response".to_string(),
        ];

        let decision = free_text_queue_head_provenance_decision(&heads, &content).unwrap();

        assert_eq!(decision.unresolved, vec!["missing response".to_string()]);
        assert!(decision.response_proven_removed.is_empty());
        assert!(decision.completed_residue.is_empty());
    }

    #[test]
    fn free_text_queue_head_provenance_decision_suppresses_marker_without_residue() {
        let decision = free_text_queue_head_provenance_decision(
            &["missing response".to_string()],
            "<!-- no-free-text-queue-head-guard -->\n\n### Re: answered\n",
        )
        .unwrap();

        assert!(decision.suppressed);
        assert!(!decision.bare_heading_residue);
        assert!(decision.unresolved.is_empty());
        assert!(decision.response_proven_removed.is_empty());
        assert!(decision.completed_residue.is_empty());
    }

    #[test]
    fn free_text_queue_head_provenance_decision_reports_bare_heading_residue() {
        let decision = free_text_queue_head_provenance_decision(
            &["missing response".to_string()],
            "<!-- no-free-text-queue-head-guard -->\n\n###\n",
        )
        .unwrap();

        assert!(decision.suppressed);
        assert!(decision.bare_heading_residue);
        assert!(decision.unresolved.is_empty());
    }

    #[test]
    fn free_text_queue_head_provenance_decision_returns_none_for_parse_failure() {
        assert_eq!(
            free_text_queue_head_provenance_decision(
                &["missing response".to_string()],
                "<!-- agent:queue -->\n- missing close\n",
            ),
            None
        );
    }

    #[test]
    fn response_head_plausibility_requires_half_of_meaningful_words() {
        assert!(response_head_plausibly_answers(
            "The churn comes from stale queue convergence.",
            "Please explain the queue churn"
        ));
        assert!(response_head_plausibly_answers(
            "Stale convergence explains the queue behavior.",
            "Please explain stale queue convergence"
        ));
        assert!(!response_head_plausibly_answers(
            "A short acknowledgement.",
            "Please explain stale queue convergence"
        ));
        assert!(!response_head_plausibly_answers(
            "Done.",
            "how does this work"
        ));
    }
}

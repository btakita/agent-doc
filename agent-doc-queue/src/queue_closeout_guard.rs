//! Pure queue-head closeout guard policy.
//!
//! This module owns id-backed queue-head decisions used by session closeout
//! guards. Callers provide document text, cycle-state facts, and tracked-work
//! id sets; file IO, cycle-state loading, ops logs, and guard-mode formatting
//! stay in orchestration.

use std::collections::HashSet;

use agent_doc_element::element;
use agent_doc_element_backlog::backlog;

use crate::{document_queue, queue_continuation, queue_directive};

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

    let mut live = Vec::new();
    for id in recorded_ids {
        let norm = normalize_id(&id);
        if norm.is_empty() {
            continue;
        }
        if !current_head_ids.contains(&norm) || !open_backlog_ids.contains(&norm) {
            continue;
        }
        if resolved_ids.contains(&norm) || deferred.contains(&norm) {
            continue;
        }
        if !live.iter().any(|existing| existing == &norm) {
            live.push(norm);
        }
    }
    live
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

fn normalize_id(id: &str) -> String {
    backlog::normalize_pending_id(id)
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
}

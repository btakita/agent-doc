//! `#managedlistnodrift`: queue rows that show `☑️` with no durable record.
//!
//! The lifecycle projection repaints a reported row whenever a claim,
//! receipt, or completion record says something else. A `☑️` that no record
//! backs is kept rather than retired, because retiring it would make finished
//! work dispatchable again (a top-level row becomes `📥`; a waiting item whose
//! packet loses it becomes selectable). This audit names every such row so
//! `repair-managed-state` and `session-check` show the operator exactly which
//! markers are unbacked; the operator removes the marker (or strikes the row)
//! to decide.

use std::path::Path;

use agent_doc_document::queue_projection::{QueueLifecycleMarker, lifecycle_marker};
use agent_doc_queue::document_queue::QueueEntry;
use anyhow::Result;

/// One `☑️` row no claim, release receipt, or lineage record backs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnbackedReportedRow {
    pub queue_id: String,
    /// The row's text, marker included.
    pub text: String,
    /// The row sits in a Waiting-on-you item's packet, where the marker is
    /// also that item's reconciliation evidence.
    pub in_waiting_packet: bool,
    /// A durable attempt with the row's own owner tag exists under a
    /// different work identity: the row's identity drifted after the attempt
    /// was recorded (an edit, a move, or a re-parented tree).
    pub owner_attempt_elsewhere: bool,
}

impl std::fmt::Display for UnbackedReportedRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text: String = self
            .text
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(140)
            .collect();
        write!(
            f,
            "agent:run id={}: `{text}` shows ☑️ but no claim, receipt, or lineage record joins its work identity{}{}",
            self.queue_id,
            if self.owner_attempt_elsewhere {
                " (the attempt its owner tag names is recorded under a different identity)"
            } else {
                ""
            },
            if self.in_waiting_packet {
                " (kept: it is the waiting item's reconciliation evidence)"
            } else {
                " (kept: retiring it would make finished work dispatchable)"
            }
        )
    }
}

/// Every live `☑️` row of `content` whose work identity joins no durable
/// attempt in `lifecycle`.
pub fn unbacked_reported_rows_in(
    content: &str,
    lifecycle: &agent_doc_work_projection::active_lifecycle::ActiveLifecycle,
) -> Result<Vec<UnbackedReportedRow>> {
    let components = agent_doc_element::element::parse(content)?;
    let set = agent_doc_queue::queue_set::QueueSet::parse(content, &components)?;
    let mut rows = Vec::new();
    for block in set.blocks() {
        for (index, entry) in block.entries.iter().enumerate() {
            let QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
            if lifecycle_marker(&prompt.text) != Some(QueueLifecycleMarker::Reported) {
                continue;
            }
            let identity = agent_doc_queue::queue_claim::claim_identity_at(&block.entries, index)
                .unwrap_or_else(|| agent_doc_queue::queue_claim::claim_identity(&prompt.text));
            if lifecycle.attempts.contains_key(&identity) {
                continue;
            }
            let owner_attempt_elsewhere = agent_doc_document::queue_projection::owner_projection(
                &prompt.text,
            )
            .is_some_and(|owner| {
                lifecycle.attempts.values().any(|attempt| {
                    attempt.owner == owner.owner && attempt.generation == owner.generation
                })
            });
            rows.push(UnbackedReportedRow {
                queue_id: block.id.to_string(),
                text: prompt.text.clone(),
                in_waiting_packet: agent_doc_queue::waiting_packet::waiting_owner(
                    &block.entries,
                    index,
                )
                .is_some(),
                owner_attempt_elsewhere,
            });
        }
    }
    Ok(rows)
}

/// [`unbacked_reported_rows_in`] against `file`'s durable state.
pub fn unbacked_reported_rows(file: &Path, content: &str) -> Result<Vec<UnbackedReportedRow>> {
    unbacked_reported_rows_in(content, &crate::active_lifecycle::load(file)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_reported_rows_without_a_record() {
        let content = concat!(
            "## Run <!-- agent:run id=\"main\" -->\n",
            "- ☑️ [#subagent:gone#2] Resolve the open operator action to complete the parent item. [#samplerelock]\n",
            "- 📥 next [#q-next]\n",
            "<!-- /agent:run -->\n",
        );
        let rows = unbacked_reported_rows_in(
            content,
            &agent_doc_work_projection::active_lifecycle::ActiveLifecycle::default(),
        )
        .unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(!rows[0].in_waiting_packet);
        assert!(
            rows[0].to_string().contains(
                "shows ☑️ but no claim, receipt, or lineage record joins its work identity"
            )
        );
    }
}

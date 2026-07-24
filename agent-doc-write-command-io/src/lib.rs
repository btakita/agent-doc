use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Durable, replayable document-mutation portion of one closeout request.
///
/// Runtime-only choices (force-disk authorization, lint overrides, origins, and
/// sibling commits) are deliberately excluded. Recovery replays the semantic
/// document intent through normal live-editor authority without inheriting an
/// unsafe transport override from the originating process.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedCloseoutMutationPlan {
    pub is_template: bool,
    pub is_stream: bool,
    pub no_pending_capture: bool,
    pub pending_add: Vec<String>,
    pub pending_add_to: Vec<String>,
    pub pending_add_gated: Vec<String>,
    pub pending_add_after: Vec<String>,
    pub pending_add_before: Vec<String>,
    pub pending_add_back: Vec<String>,
    pub backlog_queue_placement: Option<String>,
    pub icebox_add: Vec<String>,
    pub icebox_add_after: Vec<String>,
    pub icebox_add_before: Vec<String>,
    pub icebox_add_back: Vec<String>,
    pub icebox_edit: Vec<String>,
    pub icebox_clear: bool,
    pub icebox_reorder: Option<String>,
    pub pending_done: Vec<String>,
    pub pending_edit: Vec<String>,
    pub pending_clear: bool,
    pub pending_reorder: Option<String>,
    pub pending_gate: Vec<String>,
    pub pending_ungate: Vec<String>,
    pub pending_resolve_gate: Vec<String>,
    pub pending_set_gate_type: Vec<String>,
    pub pending_set_verify: Vec<String>,
    pub review_add: Vec<String>,
    pub review_edit: Vec<String>,
    pub review_remove: Vec<String>,
    pub review_resolve: Vec<String>,
    pub queue_completion_ids: Vec<String>,
    pub allow_replace_pending: bool,
    pub status: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CommandOptions {
    pub file: PathBuf,
    pub is_template: bool,
    pub is_stream: bool,
    pub is_ipc: bool,
    pub force_disk: bool,
    pub origin: Option<String>,
    /// Explicit closeout intent: the response creates no actionable follow-up work.
    /// The runtime encodes this as a transient guard marker in captured response
    /// evidence so pre-write and pre-commit checks share the same durable proof.
    pub no_pending_capture: bool,
    pub pending_add: Vec<String>,
    pub pending_add_to: Vec<String>,
    pub pending_add_gated: Vec<String>,
    /// `#ah0s`: repeated `<id> <text>` pairs - insert after the anchor id.
    pub pending_add_after: Vec<String>,
    /// `#ah0s`: repeated `<id> <text>` pairs - insert before the anchor id.
    pub pending_add_before: Vec<String>,
    /// `#ah0s`: tail-insert items (`--pending-add-back` / `--pending-append`).
    pub pending_add_back: Vec<String>,
    /// `#queueatcreate`: where items created this cycle land in `agent:queue`
    /// when the backlog opts in with a `queue` attribute — `prepend` (default,
    /// the head) or `append`. Held as the raw operator spelling so this options
    /// struct stays dependency-free; the runtime parses it.
    pub backlog_queue_placement: Option<String>,
    pub icebox_add: Vec<String>,
    /// Repeated `<id> <text>` pairs - insert after the anchor id in `agent:icebox`.
    pub icebox_add_after: Vec<String>,
    /// Repeated `<id> <text>` pairs - insert before the anchor id in `agent:icebox`.
    pub icebox_add_before: Vec<String>,
    /// Tail-insert items into `agent:icebox`.
    pub icebox_add_back: Vec<String>,
    /// Edit an icebox item: `id=new text` (repeatable).
    pub icebox_edit: Vec<String>,
    /// Clear all icebox items.
    pub icebox_clear: bool,
    /// Reorder icebox items by comma-separated hash ids.
    pub icebox_reorder: Option<String>,
    pub pending_done: Vec<String>,
    pub pending_edit: Vec<String>,
    pub pending_clear: bool,
    pub pending_reorder: Option<String>,
    pub pending_gate: Vec<String>,
    pub pending_ungate: Vec<String>,
    pub pending_resolve_gate: Vec<String>,
    pub pending_set_gate_type: Vec<String>,
    pub pending_set_verify: Vec<String>,
    pub review_add: Vec<String>,
    pub review_edit: Vec<String>,
    /// `#reviewrm`: ids to delete from `agent:review` (clears stale/duplicate
    /// entries, including same-id collisions, without an ambiguous edit-by-id).
    pub review_remove: Vec<String>,
    /// `#reviewrm`: ids to resolve out of `agent:review` into `agent:done`.
    pub review_resolve: Vec<String>,
    /// Queue-head completion ids proven by the response/route but not backed by
    /// a `pending` mutation such as `--done`.
    pub queue_completion_ids: Vec<String>,
    pub allow_replace_pending: bool,
    pub pending_only: bool,
    pub status: Option<String>,
    /// Optional CLI override for the agent-doc lint gate. `None` means
    /// "no CLI override; use frontmatter/config/default precedence".
    pub lint_override: Option<agent_doc_frontmatter::lint::LintCliMode>,
    /// Cross-repo sibling commits to run after a successful session-doc commit.
    /// Must align positionally with `commit_sibling_message`. Empty vector means
    /// "no sibling commits".
    pub commit_sibling: Vec<PathBuf>,
    /// Commit message for each `commit_sibling` entry (same length, same order).
    pub commit_sibling_message: Vec<String>,
}

impl CommandOptions {
    pub fn captured_closeout_mutation_plan(&self) -> CapturedCloseoutMutationPlan {
        CapturedCloseoutMutationPlan {
            is_template: self.is_template,
            is_stream: self.is_stream,
            no_pending_capture: self.no_pending_capture,
            pending_add: self.pending_add.clone(),
            pending_add_to: self.pending_add_to.clone(),
            pending_add_gated: self.pending_add_gated.clone(),
            pending_add_after: self.pending_add_after.clone(),
            pending_add_before: self.pending_add_before.clone(),
            pending_add_back: self.pending_add_back.clone(),
            backlog_queue_placement: self.backlog_queue_placement.clone(),
            icebox_add: self.icebox_add.clone(),
            icebox_add_after: self.icebox_add_after.clone(),
            icebox_add_before: self.icebox_add_before.clone(),
            icebox_add_back: self.icebox_add_back.clone(),
            icebox_edit: self.icebox_edit.clone(),
            icebox_clear: self.icebox_clear,
            icebox_reorder: self.icebox_reorder.clone(),
            pending_done: self.pending_done.clone(),
            pending_edit: self.pending_edit.clone(),
            pending_clear: self.pending_clear,
            pending_reorder: self.pending_reorder.clone(),
            pending_gate: self.pending_gate.clone(),
            pending_ungate: self.pending_ungate.clone(),
            pending_resolve_gate: self.pending_resolve_gate.clone(),
            pending_set_gate_type: self.pending_set_gate_type.clone(),
            pending_set_verify: self.pending_set_verify.clone(),
            review_add: self.review_add.clone(),
            review_edit: self.review_edit.clone(),
            review_remove: self.review_remove.clone(),
            review_resolve: self.review_resolve.clone(),
            queue_completion_ids: self.queue_completion_ids.clone(),
            allow_replace_pending: self.allow_replace_pending,
            status: self.status.clone(),
        }
    }

    pub fn recovery_from_captured_closeout_mutation_plan(
        file: &Path,
        plan: CapturedCloseoutMutationPlan,
    ) -> Self {
        Self {
            file: file.to_path_buf(),
            is_template: plan.is_template,
            is_stream: plan.is_stream,
            is_ipc: false,
            force_disk: false,
            origin: Some("captured_finalize_resume".to_string()),
            no_pending_capture: plan.no_pending_capture,
            pending_add: plan.pending_add,
            pending_add_to: plan.pending_add_to,
            pending_add_gated: plan.pending_add_gated,
            pending_add_after: plan.pending_add_after,
            pending_add_before: plan.pending_add_before,
            pending_add_back: plan.pending_add_back,
            backlog_queue_placement: plan.backlog_queue_placement,
            icebox_add: plan.icebox_add,
            icebox_add_after: plan.icebox_add_after,
            icebox_add_before: plan.icebox_add_before,
            icebox_add_back: plan.icebox_add_back,
            icebox_edit: plan.icebox_edit,
            icebox_clear: plan.icebox_clear,
            icebox_reorder: plan.icebox_reorder,
            pending_done: plan.pending_done,
            pending_edit: plan.pending_edit,
            pending_clear: plan.pending_clear,
            pending_reorder: plan.pending_reorder,
            pending_gate: plan.pending_gate,
            pending_ungate: plan.pending_ungate,
            pending_resolve_gate: plan.pending_resolve_gate,
            pending_set_gate_type: plan.pending_set_gate_type,
            pending_set_verify: plan.pending_set_verify,
            review_add: plan.review_add,
            review_edit: plan.review_edit,
            review_remove: plan.review_remove,
            review_resolve: plan.review_resolve,
            queue_completion_ids: plan.queue_completion_ids,
            allow_replace_pending: plan.allow_replace_pending,
            pending_only: false,
            status: plan.status,
            lint_override: None,
            commit_sibling: Vec::new(),
            commit_sibling_message: Vec::new(),
        }
    }

    pub fn repair_replay(
        file: &Path,
        is_template: bool,
        is_stream: bool,
        force_disk: bool,
        queue_completion_ids: &[String],
    ) -> Self {
        Self {
            file: file.to_path_buf(),
            is_template,
            is_stream,
            is_ipc: false,
            force_disk,
            origin: Some("repair_replay".to_string()),
            no_pending_capture: false,
            pending_add: Vec::new(),
            pending_add_to: Vec::new(),
            pending_add_gated: Vec::new(),
            pending_add_after: Vec::new(),
            pending_add_before: Vec::new(),
            pending_add_back: Vec::new(),
            backlog_queue_placement: None,
            icebox_add: Vec::new(),
            icebox_add_after: Vec::new(),
            icebox_add_before: Vec::new(),
            icebox_add_back: Vec::new(),
            icebox_edit: Vec::new(),
            icebox_clear: false,
            icebox_reorder: None,
            pending_done: Vec::new(),
            pending_edit: Vec::new(),
            pending_clear: false,
            pending_reorder: None,
            pending_gate: Vec::new(),
            pending_ungate: Vec::new(),
            pending_resolve_gate: Vec::new(),
            pending_set_gate_type: Vec::new(),
            pending_set_verify: Vec::new(),
            review_add: Vec::new(),
            review_edit: Vec::new(),
            review_remove: Vec::new(),
            review_resolve: Vec::new(),
            queue_completion_ids: queue_completion_ids.to_vec(),
            allow_replace_pending: false,
            pending_only: false,
            status: None,
            lint_override: None,
            commit_sibling: Vec::new(),
            commit_sibling_message: Vec::new(),
        }
    }

    pub fn has_pending_mutation(&self) -> bool {
        !self.pending_add.is_empty()
            || !self.pending_add_to.is_empty()
            || !self.pending_add_gated.is_empty()
            || !self.pending_add_after.is_empty()
            || !self.pending_add_before.is_empty()
            || !self.pending_add_back.is_empty()
            || !self.icebox_add.is_empty()
            || !self.icebox_add_after.is_empty()
            || !self.icebox_add_before.is_empty()
            || !self.icebox_add_back.is_empty()
            || !self.icebox_edit.is_empty()
            || self.icebox_clear
            || self.icebox_reorder.is_some()
            || !self.pending_done.is_empty()
            || !self.pending_edit.is_empty()
            || self.pending_clear
            || self.pending_reorder.is_some()
            || !self.pending_gate.is_empty()
            || !self.pending_ungate.is_empty()
            || !self.pending_resolve_gate.is_empty()
            || !self.pending_set_gate_type.is_empty()
            || !self.pending_set_verify.is_empty()
            || !self.review_add.is_empty()
            || !self.review_edit.is_empty()
            || !self.review_remove.is_empty()
            || !self.review_resolve.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitMode {
    None,
    BestEffort,
    Required,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TemplateApplyOptions {
    pub force_disk: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_mutation_plan_replays_semantics_without_runtime_authority() {
        let file = Path::new("/tmp/session.md");
        let mut options = CommandOptions::repair_replay(file, false, false, true, &[]);
        options.pending_edit = vec!["fix1=narrowed next action".to_string()];
        options.force_disk = true;
        options.lint_override = Some(agent_doc_frontmatter::lint::LintCliMode::Off);
        options.commit_sibling = vec![PathBuf::from("/tmp/sibling")];
        options.commit_sibling_message = vec!["unsafe replay".to_string()];

        let recovered = CommandOptions::recovery_from_captured_closeout_mutation_plan(
            file,
            options.captured_closeout_mutation_plan(),
        );
        assert_eq!(
            recovered.pending_edit,
            vec!["fix1=narrowed next action".to_string()]
        );
        assert!(!recovered.force_disk);
        assert!(recovered.lint_override.is_none());
        assert!(recovered.commit_sibling.is_empty());
        assert!(recovered.commit_sibling_message.is_empty());
    }
}

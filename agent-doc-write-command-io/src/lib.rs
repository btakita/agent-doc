use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct CommandOptions {
    pub file: PathBuf,
    pub baseline_file: Option<PathBuf>,
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
    pub fn repair_replay(
        file: &Path,
        is_template: bool,
        is_stream: bool,
        force_disk: bool,
        queue_completion_ids: &[String],
    ) -> Self {
        Self {
            file: file.to_path_buf(),
            baseline_file: None,
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

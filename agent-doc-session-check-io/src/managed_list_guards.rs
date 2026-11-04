//! `session-check` invariant per managed list (`#managedlistnodrift`):
//! the rendered component equals `render(durable state)`.
//!
//! A mismatch is reported with the exact list, line, expected text, and
//! observed text. This guard never heals: healing goes only through the
//! reconcile path (preflight, or `agent-doc queue repair-managed-state
//! --apply`), and that path changes only binary-owned rows.

use std::path::Path;

use agent_doc_run_context_io::{AgentDocContextExt, CycleContext};
use agent_doc_work_projection::managed_list::{ListMismatch, ManagedList};
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

/// Every list's invariant over one document cut. A list whose durable state
/// cannot be read is reported as unverifiable rather than silently passing.
pub fn managed_list_mismatches(
    file: &Path,
    content: &str,
) -> Vec<(ManagedList, Result<Option<ListMismatch>>)> {
    vec![
        (
            ManagedList::Active,
            agent_doc_work_projection::managed_list::check_list(
                ManagedList::Active,
                content,
                agent_doc_work_projection::reconcile_managed_active_items,
            ),
        ),
        (ManagedList::Todo, todo_mismatch(file, content)),
    ]
}

/// `agent:todo` == render(todo records, tracked-work index + done archive).
fn todo_mismatch(file: &Path, content: &str) -> Result<Option<ListMismatch>> {
    let links = agent_doc_element_backlog_io::backlog_cmd::todo_link_index_in_content(
        file,
        content,
        true,
        &[],
    )?;
    agent_doc_work_projection::managed_list::check_list(ManagedList::Todo, content, |content| {
        Ok(agent_doc_element::todo::project_todo(content, &links)?.content)
    })
}

/// Duplicate todo ids, reported by session-check, never merged.
pub fn todo_duplicate_id_lines(content: &str) -> Vec<String> {
    let Ok(projection) = agent_doc_element::todo::project_todo(
        content,
        &agent_doc_element::todo::TodoLinkIndex::default(),
    ) else {
        return Vec::new();
    };
    projection
        .duplicate_ids
        .into_iter()
        .map(|(id, rows)| {
            format!(
                "[session-check] warning: {} (#managedlistnodrift)",
                agent_doc_work_projection::managed_list::ReconcileAction::DuplicateId {
                    list: ManagedList::Todo,
                    id,
                    rows,
                }
            )
        })
        .collect()
}

/// Render the per-list findings as `session-check` warning lines.
pub fn managed_list_guard_lines(
    file: &Path,
    findings: Vec<(ManagedList, Result<Option<ListMismatch>>)>,
) -> Vec<String> {
    findings
        .into_iter()
        .filter_map(|(list, finding)| match finding {
            Ok(None) => None,
            Ok(Some(mismatch)) => Some(format!(
                "[session-check] warning: managed list drift (#managedlistnodrift): {mismatch}. The next preflight reconciles binary-owned rows; `agent-doc queue repair-managed-state {} --apply` runs the same reconcile now",
                file.display()
            )),
            Err(err) => Some(format!(
                "[session-check] warning: {list} invariant unverifiable (#managedlistnodrift): {err:#}"
            )),
        })
        .collect()
}

pub fn check_managed_list_invariants(file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    let content = rc.doc_content();
    let mut lines = managed_list_guard_lines(file, managed_list_mismatches(file, &content));
    lines.extend(todo_duplicate_id_lines(&content));
    Ok(if lines.is_empty() {
        GuardResult::None
    } else {
        GuardResult::Warn(lines)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_drift_reports_the_exact_line() {
        let content = concat!(
            "## Active <!-- agent:active -->\n",
            "- **Queue** (1)\n",
            "  - `[#gone]` — finished long ago\n",
            "    - **Status:** reported\n",
            "<!-- /agent:active -->\n",
            "## Run <!-- agent:run -->\n",
            "- 📥 next [#next]\n",
            "<!-- /agent:run -->\n",
        );
        let lines = managed_list_guard_lines(
            Path::new("plan.md"),
            managed_list_mismatches(Path::new("plan.md"), content),
        );
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("agent:active line 1"), "{}", lines[0]);
        assert!(
            lines[0].contains("the document has `- **Queue** (1)`"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn todo_drift_names_the_open_item_whose_work_is_done() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plan.md");
        let content = concat!(
            "## Todo <!-- agent:todo -->\n",
            "- [ ] [#shipped] Announce the release\n",
            "- [ ] [#dup] one\n",
            "- [ ] [#dup] two\n",
            "<!-- /agent:todo -->\n",
            "## Backlog <!-- agent:backlog -->\n",
            "- [x] [#shipped] Ship the release\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&file, content).unwrap();
        let lines = managed_list_guard_lines(&file, managed_list_mismatches(&file, content));
        assert!(
            lines.iter().any(|line| line.contains(
                "agent:todo line 1: render(durable state) has `- [x] [#shipped] Announce the release`"
            )),
            "{lines:?}"
        );
        let duplicates = todo_duplicate_id_lines(content);
        assert_eq!(duplicates.len(), 1, "{duplicates:?}");
        assert!(duplicates[0].contains("[#dup] names 2 rows (reported, not merged)"));
    }

    #[test]
    fn a_reconciled_document_passes() {
        let content = concat!(
            "## Active <!-- agent:active -->\n",
            "<!-- /agent:active -->\n",
            "## Run <!-- agent:run -->\n",
            "- 📥 next [#next]\n",
            "<!-- /agent:run -->\n",
        );
        let reconciled = agent_doc_work_projection::reconcile_managed_active_items(content)
            .unwrap()
            .unwrap_or_else(|| content.to_string());
        let lines = managed_list_guard_lines(
            Path::new("plan.md"),
            managed_list_mismatches(Path::new("plan.md"), &reconciled),
        );
        assert!(lines.is_empty(), "{lines:?}");
    }
}

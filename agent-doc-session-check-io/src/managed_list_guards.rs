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
        (ManagedList::Active, active_mismatch(file, content)),
        (ManagedList::Worktrees, worktrees_mismatch(file, content)),
        (ManagedList::Queue, queue_mismatch(file, content)),
        (ManagedList::Todo, todo_mismatch(file, content)),
        (
            ManagedList::Backlog,
            tracked_mismatch(ManagedList::Backlog, content),
        ),
        (
            ManagedList::Review,
            tracked_mismatch(ManagedList::Review, content),
        ),
        (
            ManagedList::Icebox,
            tracked_mismatch(ManagedList::Icebox, content),
        ),
    ]
}

/// backlog / review / icebox == render(tracked-item records).
fn tracked_mismatch(list: ManagedList, content: &str) -> Result<Option<ListMismatch>> {
    agent_doc_work_projection::managed_list::check_list(list, content, |content| {
        Ok(agent_doc_work_projection::tracked_lists::project_tracked_lists(content)?.content)
    })
}

/// Findings that are not a line of one component: duplicate ids (reported,
/// never merged) and done items not yet archived into `agent:done`.
pub fn tracked_finding_lines(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if let Ok(duplicates) = agent_doc_work_projection::tracked_lists::duplicate_open_ids(content) {
        lines.extend(
            duplicates
                .into_iter()
                .map(|action| format!("[session-check] warning: {action} (#managedlistnodrift)")),
        );
    }
    if let Ok(Some((list, item))) = agent_doc_work_projection::tracked_lists::done_drift(content) {
        lines.push(format!(
            "[session-check] warning: managed list drift (#managedlistnodrift): {} has done item `{item}` still in {list}; the next preflight archives it",
            ManagedList::Done
        ));
    }
    lines
}

/// `agent:worktrees` == render(lineage worktrees whose directory still exists).
fn worktrees_mismatch(file: &Path, content: &str) -> Result<Option<ListMismatch>> {
    agent_doc_work_projection::managed_list::check_list(
        ManagedList::Worktrees,
        content,
        |content| agent_doc_queue_io::worktree_inventory::reconcile(file, content),
    )
}

/// `agent:active` == render(claims, receipts, lineage). The age value is a
/// function of the clock, not of durable state, so it is compared as `_`.
fn active_mismatch(file: &Path, content: &str) -> Result<Option<ListMismatch>> {
    use agent_doc_work_projection::active_lifecycle::without_age_values;
    use agent_doc_work_projection::managed_list::{component_bodies, replace_component_body};
    let Some(rendered) =
        agent_doc_work_projection::reconcile_managed_active_items_for(file, content)?
    else {
        return Ok(None);
    };
    let normalize = |doc: &str| -> Result<String> {
        let Some(body) = component_bodies(doc, ManagedList::Active)?
            .first()
            .map(|body| without_age_values(body))
        else {
            return Ok(doc.to_string());
        };
        let body = if body.is_empty() || body.ends_with('\n') {
            body
        } else {
            format!("{body}\n")
        };
        Ok(replace_component_body(doc, ManagedList::Active, &body)?
            .unwrap_or_else(|| doc.to_string()))
    };
    agent_doc_work_projection::managed_list::list_mismatch(
        ManagedList::Active,
        &normalize(content)?,
        &normalize(&rendered)?,
    )
}

/// Queue lifecycle markers == render(claims, receipts, completion evidence).
fn queue_mismatch(file: &Path, content: &str) -> Result<Option<ListMismatch>> {
    // An unreadable ledger would project every row as `❔`; report it as
    // unverifiable instead of as drift.
    agent_doc_queue_io::active_lifecycle::load(file)?;
    agent_doc_work_projection::managed_list::check_list(ManagedList::Queue, content, |content| {
        let projection =
            agent_doc_queue_io::queue_cmd::derive_claim_runtime_projection(file, content, None)?;
        Ok(projection.changed.then_some(projection.target_content))
    })
}

/// `☑️` rows no durable record backs: kept (retiring them could re-run
/// finished work), and reported so the operator decides.
pub fn unbacked_reported_lines(file: &Path, content: &str) -> Vec<String> {
    match agent_doc_queue_io::queue_marker_audit::unbacked_reported_rows(file, content) {
        Ok(rows) => rows
            .into_iter()
            .map(|row| format!("[session-check] warning: {row} (#managedlistnodrift)"))
            .collect(),
        Err(err) => vec![format!(
            "[session-check] warning: agent:run reported-marker audit unverifiable (#managedlistnodrift): {err:#}"
        )],
    }
}

/// How many document commits a reported attempt may survive before it is
/// reported: enough to see the allowance exceeded, bounded so a long history
/// is never walked.
const REPORTED_CLOSEOUT_COMMIT_SCAN_LIMIT: usize = 50;

/// `#reportedcloseout`: a `Reported` (`☑️`) attempt still open after more than
/// one committed cycle. Reported as a warning with its exact next command;
/// never a failure, and never healed here (closing it is coordinator work).
pub fn overdue_reported_closeout_lines(file: &Path, content: &str) -> Vec<String> {
    use agent_doc_queue::reported_closeout::{
        overdue_reported_closeout_warning, reported_closeout_overdue, reported_since,
    };
    let closeouts = match agent_doc_queue_io::queue_claim::reported_claim_closeouts_for_content(
        file, content,
    ) {
        Ok(closeouts) => closeouts,
        Err(err) => {
            return vec![format!(
                "[session-check] warning: reported queue closeout unverifiable (#reportedcloseout): {err:#}"
            )];
        }
    };
    closeouts
        .into_iter()
        .filter_map(|(claim, closeout)| {
            let (since, _) = reported_since(&claim);
            let commits = agent_doc_git_io::revision::file_commits_since(
                file,
                since,
                REPORTED_CLOSEOUT_COMMIT_SCAN_LIMIT,
            )
            .ok()?;
            reported_closeout_overdue(commits).then(|| {
                format!(
                    "[session-check] warning: {} (#reportedcloseout)",
                    overdue_reported_closeout_warning(&claim, &closeout, commits)
                )
            })
        })
        .collect()
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
    lines.extend(tracked_finding_lines(&content));
    lines.extend(unbacked_reported_lines(file, &content));
    lines.extend(overdue_reported_closeout_lines(file, &content));
    Ok(if lines.is_empty() {
        GuardResult::None
    } else {
        GuardResult::Warn(lines)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, content).unwrap();
        (dir, file)
    }

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
        let (_dir, file) = doc_file(content);
        let lines = managed_list_guard_lines(&file, managed_list_mismatches(&file, content));
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
    fn tracked_drift_reports_the_gated_backlog_row_and_duplicates() {
        let content = concat!(
            "## Backlog <!-- agent:backlog -->\n",
            "- [ ] [#open] Open work\n",
            "- [/] [#gate] Waits on a vendor\n",
            "- [x] [#finished] Done but not archived\n",
            "<!-- /agent:backlog -->\n",
            "## Review <!-- agent:review -->\n",
            "<!-- /agent:review -->\n",
            "## Icebox <!-- agent:icebox -->\n",
            "- [ ] [#open] parked copy\n",
            "<!-- /agent:icebox -->\n",
        );
        let (_dir, file) = doc_file(content);
        let lines = managed_list_guard_lines(&file, managed_list_mismatches(&file, content));
        assert!(
            lines.iter().any(|line| line.contains(
                "agent:backlog line 2: render(durable state) has `- [x] [#finished] Done but not archived`, the document has `- [/] [#gate] Waits on a vendor`"
            )),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains(
                "agent:review line 1: render(durable state) has `- [/] [#gate] Waits on a vendor`"
            )),
            "{lines:?}"
        );
        let findings = tracked_finding_lines(content);
        assert!(
            findings
                .iter()
                .any(|line| line.contains("[#open] names 2 rows")),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|line| line.contains("agent:done has done item `[#finished] Done but not archived` still in agent:backlog")),
            "{findings:?}"
        );
    }

    /// `#reportedcloseout`: a reported attempt that survives more than one
    /// committed cycle warns with its next command; one commit is the
    /// allowance (the commit that lands the report).
    #[test]
    fn reported_attempt_surviving_two_committed_cycles_warns_with_next_command() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Closeout Test"]);
        std::fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
        let file = root.join("plan.md");
        let doc = "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue go -->\n- review the pull request\n<!-- /agent:queue -->\n";
        std::fs::write(&file, doc).unwrap();
        let item = "review the pull request";
        let reported_at = 1_700_000_000;
        let _clock =
            agent_doc_queue_io::queue_claim::pin_claim_clock_for_current_thread(reported_at);
        let (_, claimed) =
            agent_doc_queue_io::queue_claim::claim(&file, item, "subagent:review", 600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &file,
            item,
            "subagent:review",
            "worker:review",
            600,
            claimed.generation,
        )
        .unwrap();
        let reported =
            agent_doc_queue_io::queue_claim::report_attempt(&file, item, Some(claimed.generation))
                .unwrap();
        // `#reportedcloseoutfacts`: what `report-complete --prose-only` records.
        agent_doc_queue_io::queue_lineage::record_prose_only(&file, &reported, "review posted")
            .unwrap();

        let commit = |n: u64| {
            std::fs::write(&file, format!("{doc}\n<!-- cycle {n} -->\n")).unwrap();
            let date = format!("@{} +0000", reported_at + n * 60);
            let output = std::process::Command::new("git")
                .current_dir(root)
                .env("GIT_COMMITTER_DATE", &date)
                .env("GIT_AUTHOR_DATE", &date)
                .args(["commit", "-qam", &format!("cycle {n}")])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["add", ".gitignore", "plan.md"]);
        let output = std::process::Command::new("git")
            .current_dir(root)
            .env("GIT_COMMITTER_DATE", "@1600000000 +0000")
            .env("GIT_AUTHOR_DATE", "@1600000000 +0000")
            .args(["commit", "-qm", "before the report"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let content = std::fs::read_to_string(&file).unwrap();
        assert!(overdue_reported_closeout_lines(&file, &content).is_empty());
        commit(1);
        assert!(
            overdue_reported_closeout_lines(&file, &content).is_empty(),
            "the commit that lands the report is the allowance"
        );
        commit(2);
        let lines = overdue_reported_closeout_lines(&file, &content);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("[session-check] warning: "),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].contains("after 2 committed cycle(s) since reported"),
            "{}",
            lines[0]
        );
        assert!(
            lines[0].contains(&format!(
                "Next: `agent-doc queue consume {} --ack-text 'review the pull request' --ack-owner 'subagent:review#1'`",
                file.display()
            )),
            "{}",
            lines[0]
        );
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
        let (_dir, file) = doc_file(&reconciled);
        let lines = managed_list_guard_lines(&file, managed_list_mismatches(&file, &reconciled));
        assert!(lines.is_empty(), "{lines:?}");
    }
}

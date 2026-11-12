//! Load the live `agent:worktrees` inventory for one document.
//!
//! The durable source is the document's queue lineage ledger. A worktree is
//! live when its directory still exists and the attempt has no `removed`
//! receipt. The projection itself is pure and lives in
//! `agent_doc_work_projection::worktrees`.
//!
//! [`maintain`] prunes a linked worktree that is no longer needed, or that
//! the operator marked delete/remove, and records the `removed` receipt.
//! Instruction children are not performed here; preflight queues them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agent_doc_queue::queue_claim::{QueueClaimState, claim_identity};
use agent_doc_work_projection::worktrees::{
    ClaimSignal, PullRequestRef, PullRequestState, WorktreeCandidate, WorktreeInstruction,
    WorktreeOperatorChildKind, WorktreeRecord, active_records, auto_prune,
    combine_pull_request_states, named_pull_requests, operator_child_actions,
    parse_pull_request_state, queued_worktree_instruction, reconcile as reconcile_records,
    row_signal, work_status,
};
use anyhow::{Context, Result};

use crate::queue_lineage::{
    LineageWorktree, QueueLineageEventKind, RecordRequest, lineage_worktrees, load as load_lineage,
    record,
};

/// Re-project `agent:worktrees` in `content` from `file`'s lineage.
/// `None` means the component is already the inventory.
pub fn reconcile(file: &Path, content: &str) -> Result<Option<String>> {
    let records = load(file)?;
    reconcile_records(content, &records)
}

/// Live worktrees recorded against `file`, ordered by directory.
pub fn load(file: &Path) -> Result<Vec<WorktreeRecord>> {
    if agent_doc_fs::find_project_root(file).is_none() {
        return Ok(Vec::new());
    }
    let ledger = load_lineage(file)?;
    Ok(records_from_observed(
        &lineage_worktrees(&ledger),
        |directory| Path::new(directory).is_dir(),
        current_branch,
    ))
}

/// Choose the inventory from lineage observations. `directory_exists` and
/// `branch_of` are injected so the filter is testable without a git checkout.
/// `branch_of` is consulted only for a live, unremoved directory whose
/// receipt recorded no branch.
pub fn records_from_observed(
    observed: &[LineageWorktree],
    directory_exists: impl Fn(&str) -> bool,
    mut branch_of: impl FnMut(&str) -> Option<String>,
) -> Vec<WorktreeRecord> {
    let candidates = observed
        .iter()
        .map(|entry| {
            let directory = entry.directory.trim().to_string();
            let live = !entry.removed && directory_exists(&directory);
            let recorded = entry
                .branch
                .as_deref()
                .map(str::trim)
                .filter(|branch| !branch.is_empty())
                .map(str::to_string);
            let branch = recorded
                .or_else(|| live.then(|| branch_of(&directory)).flatten())
                .unwrap_or_default();
            WorktreeCandidate {
                topic: entry.item_text.clone(),
                branch,
                directory,
                generation: entry.generation,
                active: live,
            }
        })
        .collect::<Vec<_>>();
    active_records(&candidates)
}

/// One checkout [`maintain`] removed, or [`inspect`] would remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRemoval {
    pub directory: String,
    pub reason: String,
}

/// A checkout left in place, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRefusal {
    pub directory: String,
    pub reason: String,
}

/// Prune and operator-action result for one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeMaintenance {
    pub removed: Vec<WorktreeRemoval>,
    pub refused: Vec<WorktreeRefusal>,
    /// Operator instructions on checkouts that were not removed.
    pub instructions: Vec<WorktreeInstruction>,
}

/// What would be pruned or queued, without removing a checkout or writing a receipt.
pub fn inspect(file: &Path, content: &str) -> Result<WorktreeMaintenance> {
    if agent_doc_fs::find_project_root(file).is_none() {
        return Ok(WorktreeMaintenance::default());
    }
    let plan = plan(file, content, &mut github_pull_request_state)?;
    Ok(maintenance_from_plan(&plan))
}

/// Remove checkouts the plan names, then record a `removed` receipt for each
/// attempt so the inventory cannot list them again. Instruction children are
/// returned and left in the document for preflight to queue.
pub fn maintain(file: &Path, content: &str) -> Result<WorktreeMaintenance> {
    let apply_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file)?;
    maintain_under_apply_guard(file, content, &apply_guard)
}

#[doc(hidden)]
pub fn maintain_under_apply_guard(
    file: &Path,
    content: &str,
    apply_guard: &crate::queue_edit_owner::QueueEditGuard,
) -> Result<WorktreeMaintenance> {
    apply_guard.ensure_protects(file)?;
    if agent_doc_fs::find_project_root(file).is_none() {
        return Ok(WorktreeMaintenance::default());
    }
    let plan = plan(file, content, &mut github_pull_request_state)?;
    apply_plan(file, &plan)
}

/// [`maintain`] then re-project. Refusals are logged and do not fail the
/// projection. This is the preflight and repair-apply path; `session-check`
/// keeps using [`reconcile`], which does not remove a checkout.
pub fn reconcile_maintained(file: &Path, content: &str) -> Result<Option<String>> {
    let apply_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file)?;
    reconcile_maintained_under_apply_guard(file, content, &apply_guard)
}

#[doc(hidden)]
pub fn reconcile_maintained_under_apply_guard(
    file: &Path,
    content: &str,
    apply_guard: &crate::queue_edit_owner::QueueEditGuard,
) -> Result<Option<String>> {
    apply_guard.ensure_protects(file)?;
    let outcome = maintain_under_apply_guard(file, content, apply_guard)?;
    for removal in &outcome.removed {
        eprintln!(
            "[worktrees] removed {} ({})",
            removal.directory, removal.reason
        );
    }
    for refusal in &outcome.refused {
        eprintln!(
            "[worktrees] left {} in place ({})",
            refusal.directory, refusal.reason
        );
    }
    reconcile(file, content)
}

fn maintenance_from_plan(plan: &MaintenancePlan) -> WorktreeMaintenance {
    WorktreeMaintenance {
        removed: plan
            .removals
            .iter()
            .map(|removal| WorktreeRemoval {
                directory: removal.directory.clone(),
                reason: removal.reason.to_string(),
            })
            .collect(),
        refused: Vec::new(),
        instructions: plan.instructions.clone(),
    }
}

struct PlannedRemoval {
    directory: String,
    reason: &'static str,
    /// `(item text, generation)` for every attempt that still lacks a receipt.
    attempts: Vec<(String, u64)>,
    /// The directory is already gone; only the receipt is missing.
    absent: bool,
}

struct MaintenancePlan {
    removals: Vec<PlannedRemoval>,
    instructions: Vec<WorktreeInstruction>,
}

fn plan(
    file: &Path,
    content: &str,
    pr_state: &mut dyn FnMut(&PullRequestRef) -> PullRequestState,
) -> Result<MaintenancePlan> {
    let observed = lineage_worktrees(&load_lineage(file)?);
    let claims = crate::queue_claim::load_ledger(file)?;
    plan_observed(&observed, content, &claims, pr_state, Path::new)
}

fn plan_observed(
    observed: &[LineageWorktree],
    content: &str,
    claims: &agent_doc_queue::queue_claim::QueueClaimLedger,
    pr_state: &mut dyn FnMut(&PullRequestRef) -> PullRequestState,
    directory: impl Fn(&str) -> &Path,
) -> Result<MaintenancePlan> {
    let body = worktrees_body(content)?;
    let actions = operator_child_actions(&body);
    let mut grouped: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, entry) in observed.iter().enumerate() {
        if entry.removed || !usable_directory(&entry.directory) {
            continue;
        }
        grouped
            .entry(entry.directory.trim().to_string())
            .or_default()
            .push(index);
    }
    let mut removals = Vec::new();
    let mut instructions = Vec::new();
    for (dir, indexes) in grouped {
        let best = indexes
            .iter()
            .copied()
            .max_by_key(|index| observed[*index].generation)
            .expect("group is non-empty");
        let entry = &observed[best];
        let children: Vec<_> = actions
            .iter()
            .filter(|action| action.directory == dir)
            .collect();
        let operator_remove = children
            .iter()
            .any(|action| matches!(action.kind, WorktreeOperatorChildKind::Remove));
        let child_instructions: Vec<WorktreeInstruction> = children
            .iter()
            .filter_map(|action| match &action.kind {
                WorktreeOperatorChildKind::Instruction(text) => Some(WorktreeInstruction {
                    directory: dir.clone(),
                    branch: action.branch.clone(),
                    topic: action.topic.clone(),
                    text: text.clone(),
                }),
                WorktreeOperatorChildKind::Remove => None,
            })
            .collect();
        let queued = queued_worktree_instruction(content, &dir)?;
        let pr = pull_request_state_for(&entry.item_text, pr_state);
        let work = work_status(
            entry.completed,
            claim_signal(claims, &entry.item_text, entry.generation),
            row_signal(content, &entry.item_text)?,
        );
        let reason = removal_reason(operator_remove, &child_instructions, queued, pr, work);
        if let Some(reason) = reason {
            let absent = !directory(&dir).is_dir();
            removals.push(PlannedRemoval {
                directory: dir,
                reason,
                attempts: indexes
                    .into_iter()
                    .map(|index| {
                        (
                            observed[index].item_text.clone(),
                            observed[index].generation,
                        )
                    })
                    .collect(),
                absent,
            });
        } else if !operator_remove {
            instructions.extend(child_instructions);
        }
    }
    Ok(MaintenancePlan {
        removals,
        instructions,
    })
}

fn removal_reason(
    operator_remove: bool,
    instructions: &[WorktreeInstruction],
    queued: bool,
    pr: Option<PullRequestState>,
    work: agent_doc_work_projection::worktrees::WorkStatus,
) -> Option<&'static str> {
    if operator_remove {
        return Some("operator remove");
    }
    if !instructions.is_empty() || queued {
        return None;
    }
    if !auto_prune(pr, work) {
        return None;
    }
    if pr == Some(PullRequestState::Closed) {
        Some("closed pull request")
    } else {
        Some("completed work")
    }
}

fn pull_request_state_for(
    item_text: &str,
    pr_state: &mut dyn FnMut(&PullRequestRef) -> PullRequestState,
) -> Option<PullRequestState> {
    let named = named_pull_requests(item_text);
    let states: Vec<_> = named.iter().map(pr_state).collect();
    combine_pull_request_states(&states)
}

fn claim_signal(
    claims: &agent_doc_queue::queue_claim::QueueClaimLedger,
    item_text: &str,
    generation: u64,
) -> ClaimSignal {
    let identity = claim_identity(item_text);
    let mut reported = false;
    for claim in &claims.claims {
        if claim.generation != generation {
            continue;
        }
        if claim.identity != identity && claim.item_text != item_text {
            continue;
        }
        match claim.state {
            QueueClaimState::Claimed
            | QueueClaimState::Running
            | QueueClaimState::Lost
            | QueueClaimState::QuotaBlocked => return ClaimSignal::InProgress,
            QueueClaimState::Reported => reported = true,
        }
    }
    if reported {
        ClaimSignal::Reported
    } else {
        ClaimSignal::None
    }
}

fn usable_directory(directory: &str) -> bool {
    let directory = directory.trim();
    !directory.is_empty()
        && !directory.contains('`')
        && !directory.contains('\n')
        && Path::new(directory).is_absolute()
}

fn worktrees_body(content: &str) -> Result<String> {
    let components =
        agent_doc_element::element::parse(content).context("agent:worktrees: parse components")?;
    Ok(components
        .iter()
        .find(|component| component.name == agent_doc_element::element::WORKTREES_COMPONENT)
        .map(|component| component.content(content).to_string())
        .unwrap_or_default())
}

fn apply_plan(file: &Path, plan: &MaintenancePlan) -> Result<WorktreeMaintenance> {
    let mut removed = Vec::new();
    let mut refused = Vec::new();
    for removal in &plan.removals {
        let path = Path::new(&removal.directory);
        let outcome = if removal.absent {
            RemoveOutcome::Absent
        } else {
            remove_linked_worktree(path)
        };
        match outcome {
            RemoveOutcome::Removed | RemoveOutcome::Absent => {
                let mut recorded = true;
                for (item, generation) in &removal.attempts {
                    if let Err(err) = record_removed(file, item, *generation, removal.reason) {
                        recorded = false;
                        refused.push(WorktreeRefusal {
                            directory: removal.directory.clone(),
                            reason: format!("removed receipt failed: {err:#}"),
                        });
                    }
                }
                if recorded {
                    removed.push(WorktreeRemoval {
                        directory: removal.directory.clone(),
                        reason: removal.reason.to_string(),
                    });
                }
            }
            RemoveOutcome::Refused(reason) => refused.push(WorktreeRefusal {
                directory: removal.directory.clone(),
                reason,
            }),
        }
    }
    Ok(WorktreeMaintenance {
        removed,
        refused,
        instructions: plan.instructions.clone(),
    })
}

fn record_removed(file: &Path, item: &str, generation: u64, reason: &str) -> Result<()> {
    record(
        file,
        item,
        generation,
        RecordRequest {
            kind: QueueLineageEventKind::Removed,
            detail: Some(format!("worktree prune: {reason}")),
            ..RecordRequest::default()
        },
    )?;
    Ok(())
}

enum RemoveOutcome {
    Removed,
    Absent,
    Refused(String),
}

/// Remove a linked git worktree. The main checkout and a non-worktree
/// directory are refused. The branch is left in place.
fn remove_linked_worktree(directory: &Path) -> RemoveOutcome {
    if !directory.exists() {
        return RemoveOutcome::Absent;
    }
    if !directory.is_dir() {
        return RemoveOutcome::Refused("not a directory".to_string());
    }
    let Some(git_dir) = git_path(directory, "--git-dir") else {
        return RemoveOutcome::Refused("not a git worktree".to_string());
    };
    let Some(common) = git_path(directory, "--git-common-dir") else {
        return RemoveOutcome::Refused("not a git worktree".to_string());
    };
    if git_dir == common {
        return RemoveOutcome::Refused("main worktree".to_string());
    }
    let Some(repo) = common.parent() else {
        return RemoveOutcome::Refused("no repository for the worktree".to_string());
    };
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["worktree", "remove", "--force"])
        .arg(directory)
        .output();
    match output {
        Ok(output) if output.status.success() && !directory.exists() => RemoveOutcome::Removed,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let reason = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
            let reason = if reason.is_empty() {
                "git worktree remove failed".to_string()
            } else {
                reason.chars().take(200).collect()
            };
            RemoveOutcome::Refused(reason)
        }
        Err(err) => RemoveOutcome::Refused(format!("spawn git worktree remove: {err}")),
    }
}

fn git_path(directory: &Path, flag: &str) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["rev-parse", "--path-format=absolute", flag])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(text.trim());
    path.canonicalize().ok().or_else(|| {
        if path.is_absolute() {
            Some(path)
        } else {
            directory.join(path).canonicalize().ok()
        }
    })
}

fn github_pull_request_state(pr: &PullRequestRef) -> PullRequestState {
    let repo = format!("{}/{}", pr.owner, pr.repo);
    let mut command = Command::new("gh");
    command.args([
        "pr",
        "view",
        &pr.number.to_string(),
        "--repo",
        &repo,
        "--json",
        "state",
    ]);
    let Some(stdout) = command_stdout(command, Duration::from_secs(8)) else {
        return PullRequestState::Unknown;
    };
    parse_pull_request_state(&stdout)
}

fn command_stdout(mut command: Command, timeout: Duration) -> Option<String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut pipe) = child.stdout.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut stdout);
                }
                return status.success().then_some(stdout);
            }
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => return None,
        }
    }
}

fn current_branch(directory: &str) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(PathBuf::from(directory))
        .args(["branch", "--show-current"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let branch = text.trim();
    (!branch.is_empty()).then(|| branch.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::*;

    #[test]
    fn public_worktree_mutators_defer_under_apply_contention() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "body").unwrap();
        let guard = crate::queue_edit_owner::QueueEditGuard::acquire(&file).unwrap();

        for error in [
            maintain(&file, "body").unwrap_err(),
            reconcile_maintained(&file, "body").unwrap_err(),
        ] {
            assert!(
                error.to_string().contains("queue mutation deferred"),
                "{error:#}"
            );
        }
        drop(guard);
    }

    fn observed(
        topic: &str,
        branch: Option<&str>,
        directory: &str,
        generation: u64,
        removed: bool,
    ) -> LineageWorktree {
        LineageWorktree {
            item_text: topic.to_string(),
            branch: branch.map(str::to_string),
            directory: directory.to_string(),
            generation,
            removed,
            completed: false,
        }
    }

    #[test]
    fn active_worktrees_keep_the_newest_live_directory() {
        let observed = vec![
            observed("old topic", Some("old"), "/wt/b", 1, false),
            observed("newer topic\nrest", None, "/wt/b", 3, false),
            observed("gone", Some("gone"), "/wt/a", 4, true),
            observed("missing", Some("missing"), "/wt/c", 1, false),
            observed("earlier", Some("feat/kept"), "/wt/b", 2, false),
        ];
        let mut asked = Vec::new();
        let records = records_from_observed(
            &observed,
            |directory| directory != "/wt/c",
            |directory| {
                asked.push(directory.to_string());
                Some("feat/from-git".to_string())
            },
        );
        assert_eq!(
            records,
            vec![WorktreeRecord {
                topic: "newer topic rest".to_string(),
                branch: "feat/from-git".to_string(),
                directory: "/wt/b".to_string(),
            }]
        );
        assert_eq!(asked, vec!["/wt/b".to_string()]);
    }

    fn plan_for(
        entries: &[LineageWorktree],
        content: &str,
        state: PullRequestState,
    ) -> MaintenancePlan {
        let claims = agent_doc_queue::queue_claim::QueueClaimLedger::default();
        plan_observed(entries, content, &claims, &mut |_| state, |path| {
            Path::new(path)
        })
        .unwrap()
    }

    #[test]
    fn a_closed_pull_request_is_pruned_and_an_open_one_is_not() {
        let closed = observed(
            "https://github.com/example/repo/pull/837: rebase the request",
            Some("feat/rebase"),
            "/tmp/wt-closed",
            1,
            false,
        );
        let plan = plan_for(std::slice::from_ref(&closed), "", PullRequestState::Closed);
        assert_eq!(plan.removals.len(), 1);
        assert_eq!(plan.removals[0].reason, "closed pull request");
        assert!(plan.instructions.is_empty());

        let open = plan_for(std::slice::from_ref(&closed), "", PullRequestState::Open);
        assert!(open.removals.is_empty());
        assert!(
            plan_for(
                &[observed(
                    "https://github.com/example/repo/pull/837: rebase the request",
                    Some("feat/rebase"),
                    "/tmp/wt-closed",
                    1,
                    false,
                )],
                "",
                PullRequestState::Unknown,
            )
            .removals
            .is_empty()
        );
    }

    #[test]
    fn a_live_row_blocks_a_closed_pull_request_and_a_struck_row_completes_work() {
        let item = "https://github.com/example/repo/pull/9: still working on the branch";
        let entry = observed(item, Some("feat/live"), "/tmp/wt-live", 1, false);
        let live = format!("## Run <!-- agent:run -->\n- {item}\n<!-- /agent:run -->\n");
        assert!(
            plan_for(
                std::slice::from_ref(&entry),
                &live,
                PullRequestState::Closed
            )
            .removals
            .is_empty()
        );
        let struck = format!("## Run <!-- agent:run -->\n- ~~{item}~~\n<!-- /agent:run -->\n");
        let plan = plan_for(&[entry], &struck, PullRequestState::Closed);
        assert_eq!(plan.removals.len(), 1);
    }

    #[test]
    fn completed_work_without_a_pull_request_is_pruned() {
        let mut entry = observed(
            "finished the notes for the sdk document",
            Some("docs/notes"),
            "/tmp/wt-notes",
            2,
            false,
        );
        entry.completed = true;
        let plan = plan_for(std::slice::from_ref(&entry), "", PullRequestState::Open);
        assert_eq!(plan.removals.len(), 1);
        assert_eq!(plan.removals[0].reason, "completed work");
        entry.completed = false;
        assert!(
            plan_for(&[entry], "", PullRequestState::Open)
                .removals
                .is_empty(),
            "no completion signal and no pull request stays"
        );
    }

    #[test]
    fn operator_delete_removes_and_a_merge_instruction_does_not() {
        let dir = "/tmp/wt-op";
        let entry = observed(
            "https://github.com/example/repo/pull/4: rebase",
            Some("feat/rebase"),
            dir,
            1,
            false,
        );
        let delete = format!(
            concat!(
                "## Worktrees <!-- agent:worktrees -->\n",
                "- **Topic:** https://github.com/example/repo/pull/4: rebase\n",
                "  - **Branch:** feat/rebase\n",
                "  - **Directory:** `{dir}`\n",
                "  - delete\n",
                "<!-- /agent:worktrees -->\n",
            ),
            dir = dir
        );
        let plan = plan_for(
            std::slice::from_ref(&entry),
            &delete,
            PullRequestState::Open,
        );
        assert_eq!(plan.removals.len(), 1);
        assert_eq!(plan.removals[0].reason, "operator remove");
        assert!(plan.instructions.is_empty());

        let merge = delete.replace("  - delete\n", "  - merge into sdk-dev\n");
        let plan = plan_for(&[entry], &merge, PullRequestState::Closed);
        assert!(
            plan.removals.is_empty(),
            "an instruction keeps a closed pull request"
        );
        assert_eq!(plan.instructions.len(), 1);
        assert_eq!(plan.instructions[0].text, "merge into sdk-dev");
    }

    #[test]
    fn maintain_removes_a_linked_worktree_for_a_closed_pull_request() {
        let fixture = git_fixture();
        let worktree = fixture.root.parent().unwrap().join("linked");
        git(
            &fixture.root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feat/prune",
                &worktree.to_string_lossy(),
                "HEAD",
            ],
        );
        let item = "https://github.com/example/repo/pull/837: rebase the closed request";
        let doc = worktrees_doc(&fixture.doc, item, &worktree);
        std::fs::write(&fixture.doc, &doc).unwrap();
        attach_worktree(&fixture.doc, item, &fixture.root, &worktree, "feat/prune");
        // The creating claim is still running and the row is live until the
        // work is done. Release it and strike the row, which is the completed
        // closed-PR case.
        crate::queue_claim::release(&fixture.doc, item).unwrap();
        let struck = doc.replace(&format!("- {item}\n"), &format!("- ~~{item}~~\n"));
        std::fs::write(&fixture.doc, &struck).unwrap();

        let planned = plan(&fixture.doc, &struck, &mut |_| PullRequestState::Closed).unwrap();
        assert_eq!(planned.removals.len(), 1, "{:?}", planned.removals.len());
        let applied = apply_plan(&fixture.doc, &planned).unwrap();
        assert!(applied.refused.is_empty(), "{:?}", applied.refused);
        assert_eq!(applied.removed.len(), 1);
        assert!(!worktree.exists(), "the linked worktree is removed");
        assert!(
            fixture.root.join("README").exists(),
            "the main checkout stays"
        );
        let again = plan(&fixture.doc, &struck, &mut |_| PullRequestState::Closed).unwrap();
        assert!(
            again.removals.is_empty(),
            "the removed receipt keeps it from coming back"
        );
    }

    #[test]
    fn operator_delete_removes_a_linked_worktree_while_its_pull_request_is_open() {
        let fixture = git_fixture();
        let worktree = fixture.root.parent().unwrap().join("delete-me");
        git(
            &fixture.root,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feat/delete",
                &worktree.to_string_lossy(),
                "HEAD",
            ],
        );
        let item = "https://github.com/example/repo/pull/4: keep going";
        let doc = worktrees_doc(&fixture.doc, item, &worktree).replace(
            &format!("`{}`\n", worktree.display()),
            &format!("`{}`\n  - remove\n", worktree.display()),
        );
        std::fs::write(&fixture.doc, &doc).unwrap();
        attach_worktree(&fixture.doc, item, &fixture.root, &worktree, "feat/delete");
        let planned = plan(&fixture.doc, &doc, &mut |_| PullRequestState::Open).unwrap();
        assert_eq!(planned.removals.len(), 1);
        assert_eq!(planned.removals[0].reason, "operator remove");
        let applied = apply_plan(&fixture.doc, &planned).unwrap();
        assert!(applied.refused.is_empty(), "{:?}", applied.refused);
        assert!(!worktree.exists());
    }

    #[test]
    fn the_main_checkout_is_not_removed() {
        let fixture = git_fixture();
        let item = "finished the notes for the sdk document";
        let doc = worktrees_doc(&fixture.doc, item, &fixture.root);
        std::fs::write(&fixture.doc, &doc).unwrap();
        let branch = git_output(&fixture.root, &["branch", "--show-current"]);
        attach_worktree(&fixture.doc, item, &fixture.root, &fixture.root, &branch);
        crate::queue_claim::release(&fixture.doc, item).unwrap();
        let struck = doc.replace(&format!("- {item}\n"), &format!("- ~~{item}~~\n"));
        std::fs::write(&fixture.doc, &struck).unwrap();
        let planned = plan(&fixture.doc, &struck, &mut |_| PullRequestState::Unknown).unwrap();
        assert_eq!(planned.removals.len(), 1, "completed work plans a removal");
        let applied = apply_plan(&fixture.doc, &planned).unwrap();
        assert!(
            applied.removed.is_empty(),
            "the main checkout is not removed: {applied:?}"
        );
        assert_eq!(applied.refused.len(), 1);
        assert!(fixture.root.exists());
        assert!(
            plan(&fixture.doc, &struck, &mut |_| PullRequestState::Unknown)
                .unwrap()
                .removals
                .len()
                == 1,
            "a refused main checkout is not given a removed receipt"
        );
    }

    struct GitFixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        doc: PathBuf,
    }

    fn git_fixture() -> GitFixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "Worktree Test"]);
        std::fs::write(root.join(".gitignore"), ".agent-doc/\n").unwrap();
        std::fs::write(root.join("README"), "readme\n").unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "# session\n").unwrap();
        git(&root, &["add", ".gitignore", "README", "session.md"]);
        git(&root, &["commit", "-qm", "base"]);
        GitFixture {
            _dir: dir,
            root,
            doc,
        }
    }

    fn worktrees_doc(doc: &Path, item: &str, directory: &Path) -> String {
        let _ = doc;
        format!(
            concat!(
                "## Run <!-- agent:run -->\n",
                "- {item}\n",
                "<!-- /agent:run -->\n",
                "## Worktrees <!-- agent:worktrees -->\n",
                "- **Topic:** {item}\n",
                "  - **Branch:** feat\n",
                "  - **Directory:** `{directory}`\n",
                "<!-- /agent:worktrees -->\n",
            ),
            item = item,
            directory = directory.display()
        )
    }

    fn attach_worktree(doc: &Path, item: &str, repo: &Path, worktree: &Path, branch: &str) {
        let (_outcome, claim) =
            crate::queue_claim::claim(doc, item, "subagent:prune", 600).unwrap();
        let claim = crate::queue_claim::start_attempt(
            doc,
            item,
            "subagent:prune",
            "worker:prune",
            600,
            claim.generation,
        )
        .unwrap();
        crate::queue_lineage::record_started(doc, &claim).unwrap();
        record(
            doc,
            item,
            claim.generation,
            RecordRequest {
                kind: QueueLineageEventKind::Attached,
                worker_id: Some("worker:prune".to_string()),
                repo_path: Some(repo.to_path_buf()),
                worktree_path: Some(worktree.to_path_buf()),
                branch: Some(branch.to_string()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }
}

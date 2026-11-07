//! Load the live `agent:worktrees` inventory for one document.
//!
//! The durable source is the document's queue lineage ledger. A worktree is
//! live when its directory still exists and the attempt has no `removed`
//! receipt. The projection itself is pure and lives in
//! `agent_doc_work_projection::worktrees`.

use std::path::{Path, PathBuf};
use std::process::Command;

use agent_doc_work_projection::worktrees::{
    WorktreeCandidate, WorktreeRecord, active_records, reconcile as reconcile_records,
};
use anyhow::Result;

use crate::queue_lineage::{LineageWorktree, lineage_worktrees, load as load_lineage};

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
    use super::*;

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
}

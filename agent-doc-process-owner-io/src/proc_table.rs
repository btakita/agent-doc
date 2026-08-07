//! One-pass `/proc` observation shared by every process-tree query
//! (`#syncownerreactive`).
//!
//! Before this module, each ownership question re-walked `/proc` from scratch:
//! `pane_runs_other_document_owner` alone issued two full directory scans per
//! candidate pane (one for the unmanaged-harness check, one for the
//! other-document check), so a sync pass over a dozen panes paid ~24 whole-table
//! walks to answer questions about the same unchanged processes.
//!
//! The walk is now an **observation** with an explicit result type. Everything
//! above it — tree traversal, command-line classification — is a pure function
//! of that observation, which is what lets [`crate::owner_graph`] express the
//! pane -> document owner map as a derivation instead of a per-caller recompute.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One process in a pane's tree, in traversal order (the pane root is first).
///
/// `cmdline` is `None` for a process whose command line could not be read
/// (kernel threads, a process that exited between the walk and the read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeProcess {
    pub pid: String,
    pub cmdline: Option<String>,
}

/// Parent pid -> child pids, observed in a single `/proc` walk.
pub type ProcChildren = HashMap<String, Vec<String>>;

/// Process-wide TTL cache for the `/proc` parent→children map
/// (`#tmuxautosyncreactive`). A full `/proc` scan is the dominant ownership-proof
/// cost, and the process table is effectively stable over a couple of seconds, so
/// consecutive syncs (and the drift survey) share one walk instead of each paying
/// a cold ~hundreds-of-ms scan of the whole table. `refresh_process_observations`
/// (the tmux-mutation invalidation edge) forces a fresh walk via
/// [`observe_proc_children_fresh`] and updates this cache.
const PROC_CHILDREN_CACHE_TTL: Duration = Duration::from_millis(2000);
static PROC_CHILDREN_CACHE: Mutex<Option<(Instant, Arc<ProcChildren>)>> = Mutex::new(None);

/// Observe `/proc` parent→children, reusing a recent process-wide observation so
/// the cold walk is paid once per TTL window, not once per sync.
pub fn observe_proc_children() -> Arc<ProcChildren> {
    let now = Instant::now();
    if let Ok(cache) = PROC_CHILDREN_CACHE.lock()
        && let Some((observed_at, children)) = cache.as_ref()
        && now.duration_since(*observed_at) < PROC_CHILDREN_CACHE_TTL
    {
        return Arc::clone(children);
    }
    let children = observe_proc_children_uncached();
    if let Ok(mut cache) = PROC_CHILDREN_CACHE.lock() {
        *cache = Some((now, Arc::new(children)));
    }
    if let Ok(cache) = PROC_CHILDREN_CACHE.lock()
        && let Some((_, children)) = cache.as_ref()
    {
        return Arc::clone(children);
    }
    Arc::new(observe_proc_children_uncached())
}

/// Force a fresh `/proc` walk (the invalidation edge after a tmux mutation) and
/// publish it to the cache so later callers in the TTL window reuse it.
pub fn observe_proc_children_fresh() -> Arc<ProcChildren> {
    let now = Instant::now();
    let children = Arc::new(observe_proc_children_uncached());
    if let Ok(mut cache) = PROC_CHILDREN_CACHE.lock() {
        *cache = Some((now, Arc::clone(&children)));
    }
    children
}

/// Walk `/proc` once and record the parent -> children edges.
///
/// Returns an empty map when `/proc` is unavailable (non-Linux). Callers that
/// traverse from a root still see the root itself, which preserves the previous
/// behavior on those platforms: the tree collapses to `[root]` and the command
/// line comes from the `ps` fallback in [`crate::process_command`].
fn observe_proc_children_uncached() -> ProcChildren {
    let mut children: ProcChildren = HashMap::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return children;
    };
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().to_string();
        if !pid.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some(ppid) = proc_stat_ppid(&stat) else {
            continue;
        };
        children.entry(ppid).or_default().push(pid);
    }
    children
}

/// The parent pid recorded in a `/proc/<pid>/stat` line.
///
/// Split on the *last* `)` because the comm field is parenthesized and may itself
/// contain spaces and parentheses.
pub fn proc_stat_ppid(stat: &str) -> Option<String> {
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(1).map(ToOwned::to_owned)
}

/// The pids of the process tree rooted at `root_pid`, root first, cycle-safe.
pub fn tree_pids(children: &ProcChildren, root_pid: &str) -> Vec<String> {
    let root_pid = root_pid.trim();
    if root_pid.is_empty() {
        return Vec::new();
    }
    let mut pids = Vec::new();
    let mut seen = HashSet::new();
    let mut frontier = vec![root_pid.to_string()];
    while let Some(pid) = frontier.pop() {
        if !seen.insert(pid.clone()) {
            continue;
        }
        pids.push(pid.clone());
        if let Some(child_pids) = children.get(&pid) {
            for child_pid in child_pids {
                frontier.push(child_pid.to_string());
            }
        }
    }
    pids
}

/// Observe the whole process tree rooted at `root_pid`: its pids plus each one's
/// command line. This is the unit the reactive owner graph stores as a `Source`.
pub fn observe_process_tree(children: &ProcChildren, root_pid: &str) -> Vec<TreeProcess> {
    tree_pids(children, root_pid)
        .into_iter()
        .map(|pid| {
            let cmdline = crate::process_command(&pid);
            TreeProcess { pid, cmdline }
        })
        .collect()
}

/// Observe one pane's process tree from a fresh `/proc` walk. Used on the
/// unscoped pass-through path, where nothing is memoized.
pub fn observe_process_tree_uncached(root_pid: &str) -> Vec<TreeProcess> {
    observe_process_tree(&observe_proc_children(), root_pid)
}

/// The command lines in a tree observation, skipping processes whose command
/// line could not be read.
pub fn tree_cmdlines(tree: &[TreeProcess]) -> impl Iterator<Item = &str> {
    tree.iter().filter_map(|process| process.cmdline.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_stat_ppid_handles_a_parenthesized_comm() {
        assert_eq!(
            proc_stat_ppid("42 (my (weird) proc) S 7 42 42 0 -1").as_deref(),
            Some("7")
        );
    }

    #[test]
    fn tree_pids_walks_descendants_root_first_and_stops_cycles() {
        let children = ProcChildren::from([
            ("10".to_string(), vec!["20".to_string(), "30".to_string()]),
            ("20".to_string(), vec!["40".to_string()]),
            // A cycle back to the root must not loop forever.
            ("40".to_string(), vec!["10".to_string()]),
        ]);
        let pids = tree_pids(&children, "10");
        assert_eq!(pids.first().map(String::as_str), Some("10"));
        let mut sorted = pids.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["10", "20", "30", "40"]);
    }

    #[test]
    fn tree_pids_of_a_blank_root_is_empty() {
        assert!(tree_pids(&ProcChildren::new(), "   ").is_empty());
    }

    #[test]
    fn tree_cmdlines_skips_unreadable_processes() {
        let tree = vec![
            TreeProcess {
                pid: "10".to_string(),
                cmdline: Some("zsh".to_string()),
            },
            TreeProcess {
                pid: "20".to_string(),
                cmdline: None,
            },
        ];
        assert_eq!(tree_cmdlines(&tree).collect::<Vec<_>>(), vec!["zsh"]);
    }
}

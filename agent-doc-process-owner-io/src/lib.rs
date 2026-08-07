//! Process-tree owner inspection adapters.
//!
//! This crate owns process traversal and composes those
//! observations with pure controller command-line ownership policy.
//!
//! Traversal is split in two (`#syncownerreactive`): [`proc_table`] performs the
//! `/proc` observation, and [`owner_graph`] holds that observation as a `Source`
//! with the pane -> document owner map derived over it. The free functions below
//! are the stable call surface; each one reads through the graph when a scope is
//! open and observes directly when it is not.

pub mod owner_graph;
pub mod proc_table;

pub use owner_graph::{
    ProcessObservationScope, ProcessObservationStats, begin_process_observation_scope,
    process_observation_scope_stats, refresh_process_observations,
};

use agent_doc_controller::command_line::agent_doc_cmdline_is_owner;
use owner_graph::with_tree_observation;
use proc_table::{TreeProcess, tree_cmdlines};
use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// Thread-local cancellation scope for the controller's structural sync effect
// (`#tmuxautosyncreactive`).
//
// The pane-layout worker sets `(own_generation, latest_generation_handle)`
// before it calls the structural sync effect; the effect body polls
// [`structural_effect_superseded`] at phase boundaries and bails early when a
// newer layout superseded this generation, instead of running the full sync.
// The standalone `agent-doc sync` CLI never sets this, so it is unaffected.
thread_local! {
    static STRUCTURAL_EFFECT_CANCEL: RefCell<Option<(u64, Arc<AtomicU64>)>> =
        const { RefCell::new(None) };
}

/// Bind the active structural effect to its own generation and a handle to the
/// controller's latest published generation. Returns the previous binding so a
/// caller can restore it (nesting is not expected in production).
pub fn set_structural_effect_generation(
    own_generation: u64,
    latest_generation: Arc<AtomicU64>,
) -> Option<(u64, Arc<AtomicU64>)> {
    STRUCTURAL_EFFECT_CANCEL.with(|c| c.borrow_mut().replace((own_generation, latest_generation)))
}

/// Clear the active structural-effect binding.
pub fn clear_structural_effect_generation() -> Option<(u64, Arc<AtomicU64>)> {
    STRUCTURAL_EFFECT_CANCEL.with(|c| c.borrow_mut().take())
}

/// True when the active structural effect's generation has been superseded by a
/// newer published layout. Always false when no binding is set (standalone CLI).
pub fn structural_effect_superseded() -> bool {
    STRUCTURAL_EFFECT_CANCEL.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|(own, latest)| latest.load(Ordering::SeqCst) != *own)
    })
}

pub fn child_pids(parent_pid: &str) -> Vec<String> {
    proc_table::observe_proc_children()
        .get(parent_pid.trim())
        .cloned()
        .unwrap_or_default()
}

pub fn process_command(pid: &str) -> Option<String> {
    if let Some(command) = proc_process_command(pid) {
        return Some(command);
    }
    let output = Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn process_tree_contains_pid(root_pid: &str, target_pid: u32) -> bool {
    let target = target_pid.to_string();
    with_tree_observation(root_pid, |tree| {
        tree.iter().any(|process| process.pid == target)
    })
}

fn proc_process_command(pid: &str) -> Option<String> {
    let pid = pid.trim();
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let bytes = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let command = bytes
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part))
        .collect::<Vec<_>>()
        .join(" ");
    let command = command.trim();
    (!command.is_empty()).then(|| command.to_string())
}

pub fn process_is_agent_session(pid: &str) -> bool {
    let Some(cmdline) = process_command(pid) else {
        return false;
    };
    cmdline_is_agent_session(&cmdline)
}

fn cmdline_is_agent_session(cmdline: &str) -> bool {
    cmdline.contains("agent-doc")
        || cmdline.contains("claude")
        || cmdline.contains("codex")
        || cmdline.contains("opencode")
}

pub fn process_tree_has_agent_session(root_pid: &str) -> bool {
    with_tree_observation(root_pid, tree_has_agent_session)
}

pub fn process_has_agent_doc_owner_for_file(pid: &str, file_path: &str) -> bool {
    let Some(cmdline) = process_command(pid) else {
        return false;
    };
    agent_doc_cmdline_is_owner(&cmdline, file_path)
}

pub fn process_tree_has_agent_doc_owner_for_file(root_pid: &str, file_path: &str) -> bool {
    with_tree_observation(root_pid, |tree| {
        tree_has_agent_doc_owner_for_file(tree, file_path)
    })
}

pub fn process_tree_agent_doc_owner_pid_for_file(
    root_pid: &str,
    file_path: &str,
) -> Option<String> {
    with_tree_observation(root_pid, |tree| {
        tree_agent_doc_owner_pid_for_file(tree, file_path)
    })
}

// -- Pure tree-level lifts --------------------------------------------------
//
// Each takes an observed tree and answers with the controller's command-line
// policy. Splitting them out keeps the observation (`/proc`) and the decision
// separable, which is what lets the tests below drive the real predicates
// instead of a parallel traversal written for the tests.

fn tree_has_agent_session(tree: &[TreeProcess]) -> bool {
    tree_cmdlines(tree).any(cmdline_is_agent_session)
}

fn tree_has_agent_doc_owner_for_file(tree: &[TreeProcess], file_path: &str) -> bool {
    tree_cmdlines(tree).any(|cmdline| agent_doc_cmdline_is_owner(cmdline, file_path))
}

fn tree_agent_doc_owner_pid_for_file(tree: &[TreeProcess], file_path: &str) -> Option<String> {
    tree.iter()
        .find(|process| {
            process
                .cmdline
                .as_deref()
                .is_some_and(|cmdline| agent_doc_cmdline_is_owner(cmdline, file_path))
        })
        .map(|process| process.pid.clone())
}

/// Return the document bound by the first agent-doc owner process in a pane's
/// process tree. The pane root is visited before its descendants, so the
/// route-owned `agent-doc start --route-owned <document>` wrapper remains the
/// authoritative binding even when the harness below it has a less specific
/// command line.
pub fn process_tree_agent_doc_owner_document(root_pid: &str) -> Option<String> {
    owner_graph::tree_agent_doc_owner_document(root_pid)
}

pub fn process_tree_owner_document_other_than(
    root_pid: &str,
    claimed_file: &Path,
) -> Option<String> {
    owner_graph::tree_owner_document_other_than(root_pid, claimed_file)
}

pub fn process_tree_owns_other_document(root_pid: &str, claimed_file: &Path) -> bool {
    process_tree_owner_document_other_than(root_pid, claimed_file).is_some()
}

/// True when this process tree runs a bare agent-harness session that agent-doc
/// did not launch (`#bare-foreign-session-guard`).
///
/// This is the operator's own `claude`/`codex` session. It binds no `.md`, so
/// the document-ownership predicates all answer "owns nothing" — which callers
/// must not read as "free to claim or reap".
///
/// `#panehijackself`: the decision is made over the WHOLE tree
/// ([`cmdlines_are_unmanaged_harness_session`]), never per process. agent-doc
/// starts the harness as a child, so a managed pane's `claude` process on its own
/// looks unmanaged — and `any()`-ing that per-process answer classified every
/// agent-doc pane as foreign.
///
/// [`cmdlines_are_unmanaged_harness_session`]: agent_doc_controller::command_line::cmdlines_are_unmanaged_harness_session
pub fn process_tree_runs_unmanaged_harness_session(root_pid: &str) -> bool {
    owner_graph::tree_runs_unmanaged_harness_session(root_pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(entries: &[(&str, &str)]) -> Vec<TreeProcess> {
        entries
            .iter()
            .map(|(pid, cmdline)| TreeProcess {
                pid: (*pid).to_string(),
                cmdline: Some((*cmdline).to_string()),
            })
            .collect()
    }

    #[test]
    fn agent_doc_owner_for_file_matches_a_descendant_and_reports_its_pid() {
        let observed = tree(&[("10", "zsh"), ("20", "agent-doc start tasks/session.md")]);

        assert!(tree_has_agent_doc_owner_for_file(
            &observed,
            "tasks/session.md"
        ));
        assert!(!tree_has_agent_doc_owner_for_file(
            &observed,
            "tasks/other.md"
        ));
        assert_eq!(
            tree_agent_doc_owner_pid_for_file(&observed, "tasks/session.md"),
            Some("20".to_string())
        );
        assert_eq!(
            tree_agent_doc_owner_pid_for_file(&observed, "tasks/other.md"),
            None
        );
    }

    #[test]
    fn agent_session_detection_matches_any_descendant() {
        assert!(tree_has_agent_session(&tree(&[
            ("10", "zsh"),
            ("20", "claude --continue"),
        ])));
        assert!(!tree_has_agent_session(&tree(&[
            ("10", "zsh"),
            ("20", "vim"),
        ])));
    }

    #[test]
    fn a_process_whose_cmdline_could_not_be_read_is_skipped_not_matched() {
        let observed = vec![
            TreeProcess {
                pid: "10".to_string(),
                cmdline: None,
            },
            TreeProcess {
                pid: "20".to_string(),
                cmdline: Some("agent-doc start tasks/session.md".to_string()),
            },
        ];
        assert_eq!(
            tree_agent_doc_owner_pid_for_file(&observed, "tasks/session.md"),
            Some("20".to_string()),
            "an unreadable process must not shadow the real owner below it"
        );
    }
}

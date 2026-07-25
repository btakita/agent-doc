//! Process-tree owner inspection adapters.
//!
//! This crate owns process traversal and composes those
//! observations with pure controller command-line ownership policy.

use agent_doc_controller::command_line::{
    agent_doc_cmdline_is_owner, agent_doc_owner_document_from_cmdline, cmdline_owns_other_document,
    owner_document_from_cmdline,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(test)]
fn parse_child_pids(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|pid| !pid.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn child_pids(parent_pid: &str) -> Vec<String> {
    proc_children_snapshot()
        .remove(parent_pid.trim())
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
    process_tree_pids(root_pid)
        .into_iter()
        .any(|pid| pid == target_pid.to_string())
}

#[cfg(test)]
fn process_tree_contains_pid_with(
    root_pid: &str,
    target_pid: &str,
    child_pids_for: impl FnMut(&str) -> Vec<String>,
) -> bool {
    process_tree_pids_with(root_pid, child_pids_for)
        .into_iter()
        .any(|pid| pid == target_pid)
}

#[cfg(test)]
fn process_tree_pids_with(
    root_pid: &str,
    mut child_pids_for: impl FnMut(&str) -> Vec<String>,
) -> Vec<String> {
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

        for child_pid in child_pids_for(&pid) {
            let child_pid = child_pid.trim();
            if child_pid.is_empty() {
                continue;
            }
            frontier.push(child_pid.to_string());
        }
    }

    pids
}

fn process_tree_pids(root_pid: &str) -> Vec<String> {
    let root_pid = root_pid.trim();
    if root_pid.is_empty() {
        return Vec::new();
    }
    let children = proc_children_snapshot();
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

fn proc_children_snapshot() -> HashMap<String, Vec<String>> {
    let mut children = HashMap::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return children;
    };
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().to_string();
        if !pid.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let stat_path = entry.path().join("stat");
        let Ok(stat) = fs::read_to_string(stat_path) else {
            continue;
        };
        let Some(ppid) = proc_stat_ppid(&stat) else {
            continue;
        };
        children.entry(ppid).or_insert_with(Vec::new).push(pid);
    }
    children
}

fn proc_stat_ppid(stat: &str) -> Option<String> {
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(1).map(ToOwned::to_owned)
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
    process_tree_pids(root_pid).into_iter().any(|pid| {
        process_command(&pid)
            .as_deref()
            .is_some_and(cmdline_is_agent_session)
    })
}

#[cfg(test)]
fn process_tree_has_agent_session_with(
    root_pid: &str,
    child_pids_for: impl FnMut(&str) -> Vec<String>,
    mut process_command_for: impl FnMut(&str) -> Option<String>,
) -> bool {
    process_tree_pids_with(root_pid, child_pids_for)
        .into_iter()
        .any(|pid| {
            process_command_for(&pid)
                .as_deref()
                .is_some_and(cmdline_is_agent_session)
        })
}

pub fn process_has_agent_doc_owner_for_file(pid: &str, file_path: &str) -> bool {
    let Some(cmdline) = process_command(pid) else {
        return false;
    };
    agent_doc_cmdline_is_owner(&cmdline, file_path)
}

pub fn process_tree_has_agent_doc_owner_for_file(root_pid: &str, file_path: &str) -> bool {
    process_tree_pids(root_pid).into_iter().any(|pid| {
        process_command(&pid)
            .as_deref()
            .is_some_and(|cmdline| agent_doc_cmdline_is_owner(cmdline, file_path))
    })
}

pub fn process_tree_agent_doc_owner_pid_for_file(
    root_pid: &str,
    file_path: &str,
) -> Option<String> {
    process_tree_pids(root_pid).into_iter().find(|pid| {
        process_command(pid)
            .as_deref()
            .is_some_and(|cmdline| agent_doc_cmdline_is_owner(cmdline, file_path))
    })
}

/// Return the document bound by the first agent-doc owner process in a pane's
/// process tree. The pane root is visited before its descendants, so the
/// route-owned `agent-doc start --route-owned <document>` wrapper remains the
/// authoritative binding even when the harness below it has a less specific
/// command line.
pub fn process_tree_agent_doc_owner_document(root_pid: &str) -> Option<String> {
    process_tree_pids(root_pid).into_iter().find_map(|pid| {
        let cmdline = process_command(&pid)?;
        agent_doc_owner_document_from_cmdline(&cmdline)
    })
}

#[cfg(test)]
fn process_tree_agent_doc_owner_document_with(
    root_pid: &str,
    child_pids_for: impl FnMut(&str) -> Vec<String>,
    mut process_command_for: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    process_tree_pids_with(root_pid, child_pids_for)
        .into_iter()
        .find_map(|pid| {
            let cmdline = process_command_for(&pid)?;
            agent_doc_owner_document_from_cmdline(&cmdline)
        })
}

#[cfg(test)]
fn process_tree_agent_doc_owner_pid_for_file_with(
    root_pid: &str,
    file_path: &str,
    child_pids_for: impl FnMut(&str) -> Vec<String>,
    mut process_command_for: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    process_tree_pids_with(root_pid, child_pids_for)
        .into_iter()
        .find(|pid| {
            process_command_for(pid)
                .as_deref()
                .is_some_and(|cmdline| agent_doc_cmdline_is_owner(cmdline, file_path))
        })
}

#[cfg(test)]
fn process_tree_has_agent_doc_owner_for_file_with(
    root_pid: &str,
    file_path: &str,
    child_pids_for: impl FnMut(&str) -> Vec<String>,
    mut process_command_for: impl FnMut(&str) -> Option<String>,
) -> bool {
    process_tree_pids_with(root_pid, child_pids_for)
        .into_iter()
        .any(|pid| {
            process_command_for(&pid)
                .as_deref()
                .is_some_and(|cmdline| agent_doc_cmdline_is_owner(cmdline, file_path))
        })
}

pub fn process_tree_owner_document_other_than(
    root_pid: &str,
    claimed_file: &Path,
) -> Option<String> {
    let claimed = claimed_file.to_string_lossy();
    process_tree_pids(root_pid).into_iter().find_map(|pid| {
        let cmdline = process_command(&pid)?;
        cmdline_owns_other_document(&cmdline, &claimed)
            .then(|| owner_document_from_cmdline(&cmdline))?
    })
}

#[cfg(test)]
fn process_tree_owner_document_other_than_with(
    root_pid: &str,
    claimed_file: &Path,
    child_pids_for: impl FnMut(&str) -> Vec<String>,
    mut process_command_for: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    let claimed = claimed_file.to_string_lossy();
    process_tree_pids_with(root_pid, child_pids_for)
        .into_iter()
        .find_map(|pid| {
            let cmdline = process_command_for(&pid)?;
            cmdline_owns_other_document(&cmdline, &claimed)
                .then(|| owner_document_from_cmdline(&cmdline))?
        })
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
    let cmdlines: Vec<String> = process_tree_pids(root_pid)
        .into_iter()
        .filter_map(|pid| process_command(&pid))
        .collect();
    agent_doc_controller::command_line::cmdlines_are_unmanaged_harness_session(
        cmdlines.iter().map(String::as_str),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parse_child_pids_trims_and_skips_blank_lines() {
        assert_eq!(
            parse_child_pids(b" 123 \n\n456\n\t789\t\n"),
            vec!["123", "456", "789"]
        );
    }

    #[test]
    fn process_tree_contains_pid_walks_descendants_and_stops_cycles() {
        assert!(process_tree_contains_pid_with("10", "10", |_| Vec::new()));
        assert!(process_tree_contains_pid_with(
            "10",
            "40",
            |pid| match pid {
                "10" => vec!["20".to_string(), "30".to_string()],
                "20" => vec!["40".to_string()],
                _ => Vec::new(),
            }
        ));
        assert!(!process_tree_contains_pid_with(
            "10",
            "99",
            |pid| match pid {
                "10" => vec!["20".to_string()],
                "20" => vec!["10".to_string()],
                _ => Vec::new(),
            }
        ));
    }

    #[test]
    fn process_tree_has_agent_doc_owner_for_file_matches_children() {
        let commands = BTreeMap::from([
            ("10", "zsh".to_string()),
            ("20", "agent-doc start tasks/session.md".to_string()),
        ]);

        assert!(process_tree_has_agent_doc_owner_for_file_with(
            "10",
            "tasks/session.md",
            |pid| match pid {
                "10" => vec!["20".to_string()],
                _ => Vec::new(),
            },
            |pid| commands.get(pid).cloned(),
        ));
        assert!(!process_tree_has_agent_doc_owner_for_file_with(
            "10",
            "tasks/other.md",
            |pid| match pid {
                "10" => vec!["20".to_string()],
                _ => Vec::new(),
            },
            |pid| commands.get(pid).cloned(),
        ));
        assert_eq!(
            process_tree_agent_doc_owner_pid_for_file_with(
                "10",
                "tasks/session.md",
                |pid| match pid {
                    "10" => vec!["20".to_string()],
                    _ => Vec::new(),
                },
                |pid| commands.get(pid).cloned(),
            ),
            Some("20".to_string())
        );
    }

    #[test]
    fn process_tree_owner_document_prefers_route_owned_wrapper() {
        let commands = BTreeMap::from([
            (
                "10",
                "agent-doc start --route-owned /repo/tasks/selected.md".to_string(),
            ),
            ("20", "codex resume --last".to_string()),
        ]);

        assert_eq!(
            process_tree_agent_doc_owner_document_with(
                "10",
                |pid| match pid {
                    "10" => vec!["20".to_string()],
                    _ => Vec::new(),
                },
                |pid| commands.get(pid).cloned(),
            )
            .as_deref(),
            Some("/repo/tasks/selected.md"),
        );
    }

    #[test]
    fn process_tree_owner_document_other_than_returns_foreign_doc() {
        let commands = BTreeMap::from([
            ("10", "zsh".to_string()),
            ("20", "codex agent-doc tasks/foreign.md".to_string()),
        ]);

        assert_eq!(
            process_tree_owner_document_other_than_with(
                "10",
                Path::new("tasks/claimed.md"),
                |pid| match pid {
                    "10" => vec!["20".to_string()],
                    _ => Vec::new(),
                },
                |pid| commands.get(pid).cloned(),
            ),
            Some("tasks/foreign.md".to_string())
        );
    }

    #[test]
    fn process_tree_has_agent_session_matches_any_descendant() {
        let commands = BTreeMap::from([("20", "claude --continue".to_string())]);

        assert!(process_tree_has_agent_session_with(
            "10",
            |pid| match pid {
                "10" => vec!["20".to_string()],
                _ => Vec::new(),
            },
            |pid| commands.get(pid).cloned(),
        ));
    }
}

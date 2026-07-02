//! Process-tree owner inspection adapters.
//!
//! This crate owns shell/process traversal (`pgrep`, `ps`) and composes those
//! observations with pure controller command-line ownership policy.

use agent_doc_controller::command_line::{
    agent_doc_cmdline_is_owner, cmdline_owns_other_document, owner_document_from_cmdline,
};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

fn parse_child_pids(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(str::trim)
        .filter(|pid| !pid.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn child_pids(parent_pid: &str) -> Vec<String> {
    let Ok(output) = Command::new("pgrep").args(["-P", parent_pid]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_child_pids(&output.stdout)
}

pub fn process_command(pid: &str) -> Option<String> {
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
    process_tree_contains_pid_with(root_pid, &target_pid.to_string(), child_pids)
}

fn process_tree_contains_pid_with(
    root_pid: &str,
    target_pid: &str,
    child_pids_for: impl FnMut(&str) -> Vec<String>,
) -> bool {
    process_tree_pids_with(root_pid, child_pids_for)
        .into_iter()
        .any(|pid| pid == target_pid)
}

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
    process_tree_has_agent_session_with(root_pid, child_pids, process_command)
}

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
    process_tree_has_agent_doc_owner_for_file_with(root_pid, file_path, child_pids, process_command)
}

pub fn process_tree_agent_doc_owner_pid_for_file(
    root_pid: &str,
    file_path: &str,
) -> Option<String> {
    process_tree_agent_doc_owner_pid_for_file_with(root_pid, file_path, child_pids, process_command)
}

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
    process_tree_owner_document_other_than_with(root_pid, claimed_file, child_pids, process_command)
}

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

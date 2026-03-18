//! Resync — validate sessions.json against live tmux panes.
//!
//! Delegates to `tmux_router::prune()` for the core prune logic.
//! The `run()` function adds verbose output for the standalone `agent-doc resync` command.
//!
//! With `--fix`, performs additional checks:
//! 1. Kills panes that are in the wrong tmux session (vs frontmatter `tmux_session`)
//! 2. Deregisters panes running non-agent-doc processes (e.g., `corky watch`)
//!    Both actions cause the next `route` to auto-start in the correct session.

use anyhow::Result;

use crate::frontmatter;
use crate::sessions::{self, Tmux};

/// Valid process names for agent-doc panes.
const AGENT_PROCESSES: &[&str] = &["agent-doc", "claude", "node"];

/// Shells considered idle (not running an agent process).
const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];

/// A problem detected during resync --fix analysis.
#[derive(Debug)]
enum Issue {
    /// Pane is in a different tmux session than the document's frontmatter expects.
    WrongSession {
        key: String,
        file: String,
        pane: String,
        actual_session: String,
        expected_session: String,
    },
    /// Pane is running a process that is not agent-doc or claude.
    WrongProcess {
        key: String,
        file: String,
        pane: String,
        process: String,
    },
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Issue::WrongSession {
                file,
                pane,
                actual_session,
                expected_session,
                ..
            } => write!(
                f,
                "{} (pane {}) in session '{}', expected '{}'",
                file, pane, actual_session, expected_session
            ),
            Issue::WrongProcess {
                file,
                pane,
                process,
                ..
            } => write!(
                f,
                "{} (pane {}) running '{}', expected agent-doc/claude",
                file, pane, process
            ),
        }
    }
}

/// Quietly prune dead panes and deduplicate entries.
/// Called automatically before route, sync, and claim operations.
/// Returns the number of registry entries removed.
pub fn prune() -> Result<usize> {
    let tmux = Tmux::default_server();
    let registry_path = sessions::registry_path();
    let removed = tmux_router::prune(&registry_path, &tmux)?;
    if removed > 0 {
        eprintln!("resync: pruned {} stale session(s)", removed);
    }
    // Purge stash windows with idle shells, then log remaining orphans
    purge_stash_windows(&tmux);
    log_orphaned_windows(&tmux);
    Ok(removed)
}

/// Purge stash windows where all panes are idle shells.
///
/// Safe criteria:
/// 1. Window name is "stash" (never touch "claude" or user-named windows)
/// 2. ALL panes are running idle shells (not claude/agent-doc/etc.)
/// 3. Window was created more than 30 seconds ago (grace period for auto-start)
fn purge_stash_windows(tmux: &Tmux) {
    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{window_id}\t#{window_name}\t#{window_activity}",
        ])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, activity_str) = (parts[0], parts[1], parts[2]);

        // Only target "stash" windows
        if window_name != "stash" {
            continue;
        }

        // Grace period: skip if last activity was within 30 seconds
        if let Ok(activity) = activity_str.parse::<u64>()
            && now.saturating_sub(activity) < 30
        {
            continue;
        }

        // Check that ALL panes are idle shells
        let pane_output = tmux
            .cmd()
            .args([
                "list-panes",
                "-t",
                window_id,
                "-F",
                "#{pane_current_command}",
            ])
            .output();
        let pane_output = match pane_output {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        let all_idle = String::from_utf8_lossy(&pane_output.stdout)
            .lines()
            .all(|cmd| IDLE_SHELLS.contains(&cmd));

        if all_idle {
            if let Err(e) = tmux
                .cmd()
                .args(["kill-window", "-t", window_id])
                .output()
            {
                eprintln!("resync: failed to purge stash window {}: {}", window_id, e);
            } else {
                eprintln!("resync: purged stash window {} (all panes idle)", window_id);
            }
        }
    }
}

/// Log tmux windows named "claude" or "stash" whose panes are all unregistered.
/// This helps diagnose why windows become orphaned without killing them.
fn log_orphaned_windows(tmux: &Tmux) {
    let registry = sessions::load().unwrap_or_default();
    let registered_panes: std::collections::HashSet<&str> = registry
        .values()
        .map(|e| e.pane.as_str())
        .collect();

    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{window_id}\t#{window_name}\t#{session_name}",
        ])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return,
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let (window_id, window_name, session_name) = (parts[0], parts[1], parts[2]);

        if window_name != "claude" && window_name != "stash" {
            continue;
        }

        let panes = tmux.list_window_panes(window_id).unwrap_or_default();
        if panes.is_empty() {
            continue;
        }

        let all_orphaned = panes.iter().all(|p| !registered_panes.contains(p.as_str()));
        if all_orphaned {
            eprintln!(
                "resync: orphaned {} window {} in session '{}' ({} unregistered panes: {})",
                window_name,
                window_id,
                session_name,
                panes.len(),
                panes.join(", ")
            );
        }
    }
}

/// Detect issues with alive panes: wrong tmux session or wrong process.
fn detect_issues(tmux: &Tmux) -> Vec<Issue> {
    let registry = match sessions::load() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("resync: failed to load registry: {}", e);
            return Vec::new();
        }
    };
    detect_issues_in_registry(tmux, &registry)
}

/// Detect issues in a given registry (testable without disk I/O).
fn detect_issues_in_registry(tmux: &Tmux, registry: &sessions::SessionRegistry) -> Vec<Issue> {
    let mut issues = Vec::new();

    for (key, entry) in registry {
        if !tmux.pane_alive(&entry.pane) {
            continue; // Dead panes are handled by prune()
        }

        let label = if entry.file.is_empty() {
            key.as_str()
        } else {
            entry.file.as_str()
        };

        // Check 1: Is the pane running an agent-doc/claude process?
        let pane_cmd = pane_current_command(tmux, &entry.pane);
        if let Some(ref cmd) = pane_cmd
            && !AGENT_PROCESSES.contains(&cmd.as_str())
            && !IDLE_SHELLS.contains(&cmd.as_str())
        {
            issues.push(Issue::WrongProcess {
                key: key.clone(),
                file: label.to_string(),
                pane: entry.pane.clone(),
                process: cmd.clone(),
            });
            continue; // Don't also check session for wrong-process panes
        }

        // Check 2: Is the pane in the expected tmux session?
        if entry.file.is_empty() {
            continue; // Can't check frontmatter without a file path
        }

        let expected_session = match std::fs::read_to_string(&entry.file) {
            Ok(content) => match frontmatter::parse(&content) {
                Ok((fm, _)) => fm.tmux_session,
                Err(_) => None,
            },
            Err(_) => None,
        };

        if let Some(ref expected) = expected_session {
            match tmux.pane_session(&entry.pane) {
                Ok(actual) if actual != *expected => {
                    issues.push(Issue::WrongSession {
                        key: key.clone(),
                        file: label.to_string(),
                        pane: entry.pane.clone(),
                        actual_session: actual,
                        expected_session: expected.clone(),
                    });
                }
                Err(e) => {
                    eprintln!(
                        "resync: failed to query session for pane {}: {}",
                        entry.pane, e
                    );
                }
                _ => {} // Matches expected session — no issue
            }
        }
    }

    issues
}

/// Get the current command running in a tmux pane.
fn pane_current_command(tmux: &Tmux, pane_id: &str) -> Option<String> {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-p",
            "#{pane_current_command}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let cmd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if cmd.is_empty() { None } else { Some(cmd) }
}

/// Apply fixes for detected issues: kill wrong-session panes, deregister wrong-process panes.
fn apply_fixes(tmux: &Tmux, issues: &[Issue]) -> Result<usize> {
    if issues.is_empty() {
        return Ok(0);
    }

    let registry_path = sessions::registry_path();
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = sessions::load()?;
    let fixed = apply_fixes_to_registry(tmux, issues, &mut registry);

    if fixed > 0 {
        sessions::save(&registry)?;
    }
    Ok(fixed)
}

/// Apply fixes to a mutable registry (testable without disk I/O).
/// Returns number of issues fixed.
fn apply_fixes_to_registry(
    tmux: &Tmux,
    issues: &[Issue],
    registry: &mut sessions::SessionRegistry,
) -> usize {
    let mut fixed = 0;

    for issue in issues {
        match issue {
            Issue::WrongSession { key, pane, .. } => {
                // Kill the pane (next route will auto-start in correct session)
                if let Err(e) = tmux.kill_pane(pane) {
                    eprintln!("resync: failed to kill pane {}: {}", pane, e);
                    continue;
                }
                registry.remove(key);
                eprintln!("  fixed: {}", issue);
                fixed += 1;
            }
            Issue::WrongProcess { key, .. } => {
                // Just deregister — don't kill the foreign process
                registry.remove(key);
                eprintln!("  fixed: {}", issue);
                fixed += 1;
            }
        }
    }

    fixed
}

/// Verbose resync for the standalone `agent-doc resync` command.
pub fn run(fix: bool) -> Result<()> {
    let tmux = Tmux::default_server();
    let registry_path = sessions::registry_path();

    // Show what's being removed (verbose)
    let registry_before = sessions::load()?;
    let before = registry_before.len();

    let removed = tmux_router::prune(&registry_path, &tmux)?;

    if removed > 0 {
        // Show which entries were removed by diffing before/after
        let registry_after = sessions::load()?;
        eprintln!("Removed {} stale session(s):", removed);
        for (key, entry) in &registry_before {
            if !registry_after.contains_key(key) {
                let label = if entry.file.is_empty() {
                    key.as_str()
                } else {
                    entry.file.as_str()
                };
                eprintln!("  {} (pane {} removed)", label, entry.pane);
            }
        }
    } else {
        eprintln!("All {} session(s) have live panes.", before);
    }

    // Detect issues with alive panes
    let issues = detect_issues(&tmux);
    if !issues.is_empty() {
        if fix {
            eprintln!("\nFixing {} issue(s):", issues.len());
            let fixed = apply_fixes(&tmux, &issues)?;
            eprintln!("\nFixed {} of {} issue(s).", fixed, issues.len());
        } else {
            eprintln!("\nFound {} issue(s) (run with --fix to resolve):", issues.len());
            for issue in &issues {
                eprintln!("  {}", issue);
            }
        }
    } else {
        eprintln!("\nNo session/process issues detected.");
    }

    // Show current state
    let registry = sessions::load()?;
    if !registry.is_empty() {
        eprintln!("\nActive sessions:");
        for (key, entry) in &registry {
            let label = if entry.file.is_empty() {
                key.as_str()
            } else {
                entry.file.as_str()
            };
            eprintln!("  {} -> pane {}", label, entry.pane);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sessions::{IsolatedTmux, SessionEntry, SessionRegistry};

    /// Helper to create a registry entry for testing.
    fn test_entry(pane: &str, file: &str) -> SessionEntry {
        SessionEntry {
            pane: pane.to_string(),
            pid: std::process::id(),
            cwd: "/tmp".to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            file: file.to_string(),
            window: String::new(),
        }
    }

    #[test]
    fn detect_dead_pane_not_flagged_as_issue() {
        // Dead panes are handled by prune(), not detect_issues.
        // detect_issues should skip dead panes entirely.
        let iso = IsolatedTmux::new("resync-test-dead");

        let mut registry = SessionRegistry::new();
        registry.insert("dead-session".to_string(), test_entry("%99999", "test.md"));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(
            issues.is_empty(),
            "dead panes should not generate issues (handled by prune), got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detect_wrong_session_pane() {
        // A pane in tmux session "wrong" but frontmatter expects "correct"
        let iso = IsolatedTmux::new("resync-test-wrong-sess");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane in session "wrong" — must wait for shell to start
        // so pane_current_command returns "zsh"/"bash" instead of "tmux"
        let pane = iso.auto_start("wrong", &cwd).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Create a temp file with frontmatter specifying tmux_session: correct
        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(
            &doc_path,
            "---\nsession: abc-123\ntmux_session: correct\n---\n# Test\n",
        )
        .unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "abc-123".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 wrong-session issue");
        assert!(
            matches!(&issues[0], Issue::WrongSession { expected_session, actual_session, .. }
                if expected_session == "correct" && actual_session == "wrong"),
            "issue should be WrongSession with correct vs wrong, got: {}",
            &issues[0]
        );
    }

    #[test]
    fn detect_wrong_process_pane() {
        // A pane running a non-agent-doc process (e.g., "sleep")
        let iso = IsolatedTmux::new("resync-test-wrong-proc");
        let cwd = std::env::current_dir().unwrap();

        // Create a pane running "sleep" (not agent-doc/claude/node/shell)
        let output = iso
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                "test",
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "sleep",
                "60",
            ])
            .output()
            .unwrap();
        let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let mut registry = SessionRegistry::new();
        registry.insert("sess-1".to_string(), test_entry(&pane, "test.md"));

        // Give tmux a moment to register the process
        std::thread::sleep(std::time::Duration::from_millis(200));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert_eq!(issues.len(), 1, "should detect 1 wrong-process issue");
        assert!(
            matches!(&issues[0], Issue::WrongProcess { process, .. } if process == "sleep"),
            "issue should be WrongProcess(sleep), got: {}",
            &issues[0]
        );
    }

    #[test]
    fn fix_wrong_session_kills_pane_and_deregisters() {
        let iso = IsolatedTmux::new("resync-test-fix-sess");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-fix".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongSession {
            key: "sess-fix".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            actual_session: "wrong".to_string(),
            expected_session: "correct".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry);
        assert_eq!(fixed, 1);
        assert!(!registry.contains_key("sess-fix"), "entry should be removed from registry");
        assert!(!iso.pane_alive(&pane), "pane should be killed");
    }

    #[test]
    fn fix_wrong_process_deregisters_but_keeps_pane() {
        let iso = IsolatedTmux::new("resync-test-fix-proc");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();
        assert!(iso.pane_alive(&pane));

        let mut registry = SessionRegistry::new();
        registry.insert("sess-proc".to_string(), test_entry(&pane, "test.md"));

        let issues = vec![Issue::WrongProcess {
            key: "sess-proc".to_string(),
            file: "test.md".to_string(),
            pane: pane.clone(),
            process: "corky".to_string(),
        }];

        let fixed = apply_fixes_to_registry(&iso, &issues, &mut registry);
        assert_eq!(fixed, 1);
        assert!(!registry.contains_key("sess-proc"), "entry should be removed from registry");
        assert!(iso.pane_alive(&pane), "pane should NOT be killed (foreign process)");
    }

    #[test]
    fn no_fix_without_flag() {
        // detect_issues returns issues but apply_fixes is only called with --fix.
        // This test verifies the reporting path doesn't mutate anything.
        let iso = IsolatedTmux::new("resync-test-no-fix");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("wrong", &cwd).unwrap();

        let tmp = tempfile::TempDir::new().unwrap();
        let doc_path = tmp.path().join("test.md");
        std::fs::write(
            &doc_path,
            "---\nsession: abc\ntmux_session: correct\n---\n",
        )
        .unwrap();

        let mut registry = SessionRegistry::new();
        registry.insert(
            "abc".to_string(),
            test_entry(&pane, &doc_path.to_string_lossy()),
        );

        // detect_issues finds the problem
        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(!issues.is_empty(), "should detect issues");

        // But without calling apply_fixes, nothing changes
        assert!(registry.contains_key("abc"), "registry should be unchanged");
        assert!(iso.pane_alive(&pane), "pane should still be alive");
    }

    #[test]
    fn healthy_pane_has_no_issues() {
        // A pane running a shell (idle) with no tmux_session mismatch should be clean.
        let iso = IsolatedTmux::new("resync-test-healthy");
        let cwd = std::env::current_dir().unwrap();

        let pane = iso.auto_start("test", &cwd).unwrap();

        // Wait for the shell to fully start (otherwise pane_current_command
        // may return "tmux" instead of "zsh"/"bash")
        std::thread::sleep(std::time::Duration::from_millis(500));

        // No file path means no frontmatter check; shell is in IDLE_SHELLS
        let mut registry = SessionRegistry::new();
        registry.insert("healthy-sess".to_string(), test_entry(&pane, ""));

        let issues = detect_issues_in_registry(&iso, &registry);
        assert!(
            issues.is_empty(),
            "healthy idle shell should have no issues, got: {:?}",
            issues.iter().map(|i| i.to_string()).collect::<Vec<_>>()
        );
    }
}

//! # Module: install
//!
//! ## Spec
//! - `check_prereqs()`: resolves the exact tmux client, reports its version, and
//!   verifies a live-server protocol handshake; also checks `claude` in the caller PATH.
//!   Never fails — warnings only. A handshake that fails only because no tmux
//!   server is running yet ("no server running" / "error connecting to ... (No
//!   such file or directory)") is reported `ok`: a fresh machine has no server
//!   and agent-doc starts one on first use. Any other handshake failure is `BROKEN`.
//! - `run(editor, skip_prereqs, skip_plugins)`: orchestrates the full install workflow.
//!   - Runs `check_prereqs()` unless `skip_prereqs` is set.
//!   - Skips plugin installation when `skip_plugins` is set.
//!   - If `editor` is given, installs only for that editor; otherwise auto-detects installed editors
//!     via `detect_editors()`.
//!   - `detect_editors()` (private): detects JetBrains by checking
//!     `~/.local/share/JetBrains/` (Linux) or `/Applications/IntelliJ*` (macOS); detects VS Code
//!     family by probing `cursor`, `codium`, or `code` in PATH.
//!   - For each detected editor, calls `crate::plugin::install(editor)` and collects
//!     installed/failed lists.
//!   - Prints a summary of installed and failed plugins to stderr.
//!
//! ## Agentic Contracts
//! - `run` always returns `Ok(())`; individual plugin failures are logged but do not propagate.
//! - `check_prereqs` is side-effect-free beyond stderr output.
//! - When no editors are detected and none is specified, installation is skipped with a hint to use
//!   `--editor`.
//!
//! ## Evals
//! - fresh_machine_without_a_tmux_server_is_not_reported_broken: absent-server errors → `ok`
//! - a_failed_handshake_with_a_live_server_is_still_broken: other handshake errors → `BROKEN` + `tmux_bin` hint
//! - check_prereqs_runs_without_panic: calling `check_prereqs()` on any system completes without panic
//! - run_skip_plugins: `skip_plugins=true` → no `plugin::install` called, returns Ok
//! - run_explicit_editor: `editor=Some("jetbrains")` → only JetBrains plugin install attempted
//! - run_no_editors_detected: empty PATH + no JetBrains dirs → skips plugin install, returns Ok

use anyhow::Result;
use std::path::PathBuf;

/// Check if a binary exists in PATH using `which`.
fn which(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// True when a tmux server handshake failed only because no server is running
/// yet. That is the normal state on a fresh machine (agent-doc starts its own
/// server on first use), so it must not be reported as a broken install.
pub(crate) fn tmux_server_not_running(error: &str) -> bool {
    error.contains("no server running")
        || (error.contains("error connecting to") && error.contains("No such file or directory"))
}

/// The prerequisite report lines for one tmux probe.
pub(crate) fn tmux_prereq_lines(tmux: &crate::env_cmd::TmuxProbe) -> Vec<String> {
    match (&tmux.version, tmux.server_handshake) {
        (Some(version), true) => vec![format!(
            "[install] tmux ... ok (binary={} client={} server={})",
            tmux.binary,
            version,
            tmux.server_version.as_deref().unwrap_or("unknown")
        )],
        (Some(version), false)
            if tmux
                .server_error
                .as_deref()
                .is_some_and(tmux_server_not_running) =>
        {
            vec![format!(
                "[install] tmux ... ok (binary={} client={}; no server running yet, agent-doc starts one when needed)",
                tmux.binary, version
            )]
        }
        (Some(version), false) => vec![
            format!(
                "[install] tmux ... BROKEN (binary={} client={})",
                tmux.binary, version
            ),
            format!(
                "[install]   cause: {}",
                tmux.server_error
                    .as_deref()
                    .unwrap_or("server handshake failed")
            ),
            "[install]   hint: set tmux_bin in .agent-doc/config.toml or ~/.config/agent-doc/config.toml to the client that owns the running server".to_string(),
        ],
        (None, _) => vec![
            format!("[install] tmux ... MISSING (binary={})", tmux.binary),
            format!(
                "[install]   cause: {}",
                tmux.version_error
                    .as_deref()
                    .unwrap_or("version probe failed")
            ),
            "[install]   hint: Install tmux: https://github.com/tmux/tmux/wiki/Installing"
                .to_string(),
        ],
    }
}

/// Check prerequisites and print status. Does not fail — only warns.
pub fn check_prereqs() {
    let tmux = crate::env_cmd::probe_tmux();
    for line in tmux_prereq_lines(&tmux) {
        eprintln!("{line}");
    }

    if which("claude") {
        eprintln!("[install] claude ... ok");
    } else {
        eprintln!("[install] claude ... MISSING in caller PATH");
        eprintln!(
            "[install]   hint: Install Claude Code CLI or verify it is available in the tmux pane shell; this check does not inspect interactive-shell PATH setup"
        );
    }
}

/// Detect which editors are installed and return their names.
fn detect_editors() -> Vec<&'static str> {
    let mut editors = Vec::new();

    // JetBrains: check for ~/.local/share/JetBrains/ (Linux) or /Applications/IntelliJ* (macOS)
    let jetbrains_found = {
        let home = std::env::var("HOME").unwrap_or_default();
        let linux_path = PathBuf::from(&home).join(".local/share/JetBrains");
        let macos_path = PathBuf::from("/Applications");

        let linux_ok = linux_path.is_dir()
            && std::fs::read_dir(&linux_path)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false);

        let macos_ok = macos_path.is_dir()
            && std::fs::read_dir(&macos_path)
                .map(|d| {
                    d.flatten()
                        .any(|e| e.file_name().to_string_lossy().starts_with("IntelliJ"))
                })
                .unwrap_or(false);

        linux_ok || macos_ok
    };

    if jetbrains_found {
        editors.push("jetbrains");
    }

    // VS Code / Cursor / Codium
    if which("cursor") || which("codium") || which("code") {
        editors.push("vscode");
    }

    editors
}

/// Main entry point for `agent-doc install`.
pub fn run(editor: Option<&str>, skip_prereqs: bool, skip_plugins: bool) -> Result<()> {
    if !skip_prereqs {
        check_prereqs();
    }

    if skip_plugins {
        eprintln!("[install] Skipping plugin installation (--skip-plugins).");
        return Ok(());
    }

    let editors_to_install: Vec<&str> = if let Some(e) = editor {
        vec![e]
    } else {
        detect_editors()
    };

    if editors_to_install.is_empty() {
        eprintln!("[install] No supported editors detected. Skipping plugin installation.");
        eprintln!("[install]   To install manually: agent-doc install --editor jetbrains|vscode");
        return Ok(());
    }

    let mut installed = Vec::new();
    let mut failed = Vec::new();

    for ed in &editors_to_install {
        eprintln!("[install] Installing plugin for {} ...", ed);
        match crate::plugin::install(ed) {
            Ok(()) => installed.push(*ed),
            Err(e) => {
                eprintln!("[install] Plugin install failed for {}: {:#}", ed, e);
                failed.push(*ed);
            }
        }
    }

    // Summary
    eprintln!("[install] ---");
    if !installed.is_empty() {
        eprintln!("[install] Installed plugins: {}", installed.join(", "));
    }
    if !failed.is_empty() {
        eprintln!("[install] Failed plugins: {}", failed.join(", "));
    }
    if installed.is_empty() && failed.is_empty() {
        eprintln!("[install] Nothing to install.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{tmux_prereq_lines, tmux_server_not_running};
    use crate::env_cmd::TmuxProbe;

    fn probe(server_error: Option<&str>) -> TmuxProbe {
        TmuxProbe {
            binary: "/usr/bin/tmux".to_string(),
            source: "path".to_string(),
            version: Some("tmux 3.5".to_string()),
            version_error: None,
            server_handshake: false,
            server_version: None,
            server_error: server_error.map(str::to_string),
        }
    }

    #[test]
    fn fresh_machine_without_a_tmux_server_is_not_reported_broken() {
        // Both shapes tmux prints when no server has been started yet. A new
        // user's first `agent-doc init` used to answer them with "BROKEN" and
        // advice to change `tmux_bin`, which is wrong for a clean machine.
        for error in [
            "/usr/bin/tmux server handshake exited Some(1): no server running on /tmp/tmux-1000/default",
            "/usr/bin/tmux server handshake exited Some(1): error connecting to /tmp/tmux-1000/default (No such file or directory)",
        ] {
            assert!(tmux_server_not_running(error), "{error}");
            let lines = tmux_prereq_lines(&probe(Some(error)));
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert!(lines[0].starts_with("[install] tmux ... ok"), "{lines:?}");
            assert!(lines[0].contains("no server running yet"), "{lines:?}");
        }
    }

    #[test]
    fn a_failed_handshake_with_a_live_server_is_still_broken() {
        let error = "/usr/bin/tmux server handshake exited Some(1): protocol version mismatch (client 8, server 7)";
        assert!(!tmux_server_not_running(error));
        let lines = tmux_prereq_lines(&probe(Some(error)));
        assert!(lines[0].contains("BROKEN"), "{lines:?}");
        assert!(
            lines.iter().any(|line| line.contains("tmux_bin")),
            "{lines:?}"
        );
    }

    #[test]
    fn missing_tmux_is_reported_missing() {
        let mut missing = probe(None);
        missing.version = None;
        missing.version_error = Some("failed to spawn tmux".to_string());
        let lines = tmux_prereq_lines(&missing);
        assert!(lines[0].contains("MISSING"), "{lines:?}");
    }
}

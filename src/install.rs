//! # Module: install
//!
//! ## Spec
//! - `check_prereqs()`: checks for `tmux` and `claude` in `PATH` via `which`, printing `ok` or
//!   `MISSING` with an install hint for each.  Never fails — warnings only.
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

/// Check prerequisites and print status. Does not fail — only warns.
pub fn check_prereqs() {
    let prereqs = [
        ("tmux", "Install tmux: https://github.com/tmux/tmux/wiki/Installing"),
        ("claude", "Install Claude Code CLI: https://docs.anthropic.com/en/docs/claude-code"),
    ];

    for (bin, install_hint) in &prereqs {
        if which(bin) {
            eprintln!("[install] {} ... ok", bin);
        } else {
            eprintln!("[install] {} ... MISSING", bin);
            eprintln!("[install]   hint: {}", install_hint);
        }
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

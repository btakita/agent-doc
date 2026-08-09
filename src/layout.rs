//! # Module: layout
//!
//! Arrange tmux panes to mirror the editor's split layout.
//!
//! Usage: `agent-doc layout <file1.md> <file2.md> [--split h|v]`
//!
//! ## Spec
//! - `Split` enum: `Horizontal` (side-by-side, `-h`) or `Vertical` (stacked, `-v`).
//! - `run(files, split, pane, window)`: entry point; delegates to `run_with_tmux` using
//!   the default tmux server.
//! - `run_with_tmux(files, split, pane, window, tmux)`:
//!   - Requires at least one file; errors if `files` is empty.
//!   - With exactly one file: delegates entirely to `agent-doc-focus-io` and returns.
//!   - For each file: reads frontmatter to obtain the session UUID via
//!     `frontmatter::ensure_session`, then looks up the live pane in the durable registry.
//!     Files with no registered pane or a dead pane are skipped with a stderr warning.
//!   - When `--window` is supplied, discards any resolved pane that does not belong to
//!     that window (prevents cross-window pane migration).
//!   - If fewer than two live, unique panes remain after filtering, focuses the first
//!     file's pane if it was resolved; returns `Ok(())` without rearranging.
//!   - Deduplicates panes so multiple files sharing a pane count as one; errors if
//!     deduplication leaves fewer than two panes.
//!   - Selects the target window by choosing the window that already contains the most
//!     wanted panes (tiebreaker: most total panes), minimising disruption to any
//!     existing layout.
//!   - Breaks out session-registered panes that are in the target window but not in the
//!     wanted set (`tmux break-pane`); non-session panes (shells, tools) are untouched.
//!     A pane running a live agent turn is moved to the tracked `stash` window instead of
//!     its own window, but it is never left visible: breaking a pane out is
//!     non-destructive, so a busy session survives the move and must not be allowed to
//!     grow a two-document projection into three panes (`#run3rdpaneswitch`).
//!   - Joins each wanted pane that is outside the target window into it via `join-pane`
//!     with the `Split` flag.
//!   - Focuses the first file's pane after the layout is complete.
//!
//! ## Agentic Contracts
//! - Only panes that are registered in the durable registry are ever broken out of the target
//!   window; unmanaged panes are never touched.
//! - A single-file invocation never modifies tmux window structure; it is a pure focus
//!   operation.
//! - The `--window` filter is a hard boundary: panes outside the specified window are
//!   silently excluded rather than migrated into it.
//! - After a successful multi-pane layout, the first file's pane is always focused.
//! - `run_with_tmux` does not modify the durable registry; session registry updates are the
//!   responsibility of `claim.rs` / `route.rs`.
//!
//! ## Evals
//! - `layout_two_files_horizontal` (aspirational): two files each with a live pane in
//!   different windows → both panes joined into one window side-by-side and first pane
//!   focused.
//! - `layout_single_file_delegates_to_focus` (aspirational): one file supplied → focus
//!   is called, no `join-pane` or `break-pane` issued.
//! - `layout_skips_dead_pane` (aspirational): one of two files has a dead pane → dead
//!   pane is skipped with a warning; if only one live pane remains, focus is called
//!   instead of rearranging.
//! - `layout_window_filter_excludes_foreign_panes` (aspirational): `--window` supplied
//!   and one resolved pane is in a different window → that pane is filtered out before
//!   arrangement.
//! - `layout_does_not_break_nonregistered_panes` (aspirational): target window contains
//!   a shell pane not in the registry alongside a registered pane to be broken out →
//!   only the registered pane is broken out; the shell pane remains.
//! - `layout_empty_files_errors` (aspirational): `files` is empty → `anyhow::bail!`
//!   with "at least one file required".
//! - `a_busy_unwanted_pane_is_stashed_never_left_visible`: eligible + busy → `Stash`,
//!   never `Leave` — the `#run3rdpaneswitch` third-pane regression.
//! - `switching_documents_while_the_outgoing_turn_runs_stays_two_panes`: live tmux, a
//!   registered busy pane plus two wanted panes → the busy pane leaves the target
//!   window into a `stash` window, stays alive, and the window holds exactly the
//!   wanted panes.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_frontmatter::frontmatter;
use tmux_router::{PaneMoveOp, Tmux};

/// Split direction for the mirror window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    /// Horizontal split (panes side by side).
    Horizontal,
    /// Vertical split (panes stacked).
    Vertical,
}

impl Split {
    fn tmux_flag(&self) -> &str {
        match self {
            Split::Horizontal => "-h",
            Split::Vertical => "-v",
        }
    }
}

pub fn run(files: &[&Path], split: Split, pane: Option<&str>, window: Option<&str>) -> Result<()> {
    run_with_tmux(files, split, pane, window, &Tmux::default_server())
}

pub fn run_with_tmux(
    files: &[&Path],
    split: Split,
    pane: Option<&str>,
    window: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    run_with_tmux_and_busy_probe(files, split, pane, window, tmux, &is_pane_busy)
}

/// [`run_with_tmux`] with the "is this pane running a live agent turn" probe
/// injected.
///
/// The real probe walks the pane's process tree, which a test cannot stage
/// without launching an actual agent. The `#run3rdpaneswitch` regression is
/// entirely about what happens to a pane the probe says is BUSY, so the seam
/// exists to let that case be exercised against real tmux.
pub(crate) fn run_with_tmux_and_busy_probe(
    files: &[&Path],
    split: Split,
    pane: Option<&str>,
    window: Option<&str>,
    tmux: &Tmux,
    pane_busy: &dyn Fn(&Tmux, &str) -> bool,
) -> Result<()> {
    tracing::debug!(file_count = files.len(), split = ?split, window, "layout::run start");
    if files.is_empty() {
        anyhow::bail!("at least one file required");
    }

    if files.len() == 1 {
        // Single file — just focus it, no layout needed.
        return agent_doc_focus_io::run_with_tmux(
            &crate::focus_effects::FOCUS_EFFECTS,
            files[0],
            pane,
            tmux,
        );
    }

    // Resolve each file to its session pane.
    let mut pane_files: Vec<(String, String)> = Vec::new(); // (pane_id, file_display)
    for file in files {
        if !file.exists() {
            anyhow::bail!("file not found: {}", file.display());
        }
        let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "layout_command_document",
        )
        .with_context(|| format!("failed to resolve {}", file.display()))?;
        let (_updated, session_id) = frontmatter::ensure_session(&content)?;
        let pane = agent_doc_session_registry_io::lookup(&session_id)?;
        match pane {
            Some(pane_id) if tmux.pane_alive(&pane_id) => {
                pane_files.push((pane_id, file.display().to_string()));
            }
            Some(pane_id) => {
                eprintln!(
                    "warning: pane {} is dead for {}, skipping",
                    pane_id,
                    file.display()
                );
            }
            None => {
                eprintln!(
                    "warning: no pane registered for {}, skipping",
                    file.display()
                );
            }
        }
    }

    // If --window is specified, filter to only panes in that window.
    // This prevents layout from pulling panes from other windows.
    if let Some(win) = window {
        let window_panes_list = tmux.list_window_panes(win).unwrap_or_default();
        let window_pane_set: std::collections::HashSet<&str> =
            window_panes_list.iter().map(|s| s.as_str()).collect();
        let before = pane_files.len();
        pane_files.retain(|(pane_id, _)| window_pane_set.contains(pane_id.as_str()));
        if pane_files.len() < before {
            eprintln!(
                "Filtered {} panes outside window {}",
                before - pane_files.len(),
                win
            );
        }
    }

    if pane_files.len() < 2 {
        // Only focus the most recently selected file's pane (files[0]).
        // If that file has no pane, don't change focus at all — the user
        // selected an unclaimed file, so switching to a different pane
        // would be confusing.
        if let Some(first_file) = files.first() {
            let first_display = first_file.display().to_string();
            for (pane_id, display) in &pane_files {
                if *display == first_display {
                    tmux.select_pane(pane_id)?;
                    break;
                }
            }
        }
        return Ok(());
    }

    // Deduplicate panes (multiple files might share a pane).
    let mut seen = std::collections::HashSet::new();
    pane_files.retain(|(pane_id, _)| seen.insert(pane_id.clone()));

    if pane_files.len() < 2 {
        anyhow::bail!("all files share the same pane — nothing to arrange");
    }

    // Collect the set of wanted pane IDs.
    let wanted: std::collections::HashSet<&str> =
        pane_files.iter().map(|(id, _)| id.as_str()).collect();

    // Pick the target window — the one containing the most wanted panes.
    // Tiebreaker: prefer the window with the most total panes (the existing
    // layout window). This keeps the current layout in place and swaps panes
    // in/out, rather than moving everything to a solo pane's window.
    let mut best_window = String::new();
    let mut best_wanted = 0usize;
    let mut best_total = 0usize;
    let mut anchor_pane = pane_files[0].0.clone(); // fallback
    for (pane_id, _) in &pane_files {
        let window = tmux.pane_window(pane_id)?;
        let window_panes = tmux.list_window_panes(&window)?;
        let wanted_count = window_panes
            .iter()
            .filter(|p| wanted.contains(p.as_str()))
            .count();
        let total = window_panes.len();
        if wanted_count > best_wanted || (wanted_count == best_wanted && total > best_total) {
            best_wanted = wanted_count;
            best_total = total;
            best_window = window;
            anchor_pane = pane_id.clone();
        }
    }
    let target_window = best_window;

    // Break out unwanted panes, but only if they are registered sessions.
    // Non-session panes (shells, tools, etc.) are left in place — the user
    // didn't ask us to manage them.
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    let session_panes: std::collections::HashSet<String> =
        registry.values().map(|e| e.pane.clone()).collect();

    let window_panes = tmux.list_window_panes(&target_window)?;
    for existing_pane in &window_panes {
        let eligible = !wanted.contains(existing_pane.as_str())
            && session_panes.contains(existing_pane)
            && window_panes.len() > 1;
        // Short-circuits so the process-tree walk only runs for a pane that is
        // actually going to move.
        let busy = eligible && pane_busy(tmux, existing_pane);
        match unwanted_pane_disposition(eligible, busy) {
            UnwantedPaneDisposition::Leave => {}
            UnwantedPaneDisposition::Stash => {
                let session = tmux.pane_session(existing_pane).unwrap_or_default();
                tmux.break_pane_to_stash(existing_pane, &session)?;
                eprintln!(
                    "Stashed busy pane {} from window {}",
                    existing_pane, target_window
                );
            }
            UnwantedPaneDisposition::BreakOut => {
                tmux.break_pane(existing_pane)?;
                eprintln!(
                    "Broke out pane {} from window {}",
                    existing_pane, target_window
                );
            }
        }
    }

    // Join remaining panes into the target window with the requested split.
    for (pane_id, file_display) in &pane_files {
        let pane_window = tmux.pane_window(pane_id)?;
        if pane_window == target_window {
            continue;
        }

        PaneMoveOp::new(tmux, pane_id, &anchor_pane).join(split.tmux_flag())?;
        eprintln!(
            "Joined {} (pane {}) into window {}",
            file_display, pane_id, target_window
        );
    }

    // Focus the first file's pane (the most recently selected file from the plugin).
    let (focus_pane, _) = &pane_files[0];
    tmux.select_pane(focus_pane)?;

    eprintln!(
        "Layout: {} panes arranged {}",
        pane_files.len(),
        match split {
            Split::Horizontal => "side-by-side",
            Split::Vertical => "stacked",
        }
    );
    Ok(())
}

/// What `arrange` does with a pane that is already sitting in the target window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnwantedPaneDisposition {
    /// Wanted, unmanaged, or the last pane in the window — leave it alone.
    Leave,
    /// Registered and idle: move it to its own window.
    BreakOut,
    /// Registered and running a live agent turn: move it to the tracked `stash`
    /// window, where `sync` can rescue it later.
    Stash,
}

/// `#run3rdpaneswitch`: a busy pane must still leave the visible projection.
///
/// This used to `continue` on a busy pane — "skip busy panes (running
/// agent-doc/claude sessions)" — which left it visible while the incoming
/// document's pane joined the same window below. Switching documents while the
/// outgoing turn was still running therefore produced a THIRD pane in a
/// two-column editor projection, holding the previous session with its prompt
/// submitted and its turn running. Operator-reported 2026-08-09.
///
/// Skipping was never what protected the session. `break_pane` moves a pane to
/// another window; the process and its turn keep running either way, so
/// visibility and survival are independent. `sync.rs` already settled this for
/// open-cycle panes — "Stashing is non-destructive, so the closeout keeps
/// running without forcing a third visible pane into a two-document editor
/// projection" — and layout now mirrors it, routing the busy pane to the
/// tracked `stash` window rather than an untracked orphan so a later sync can
/// bring it back.
pub(crate) const fn unwanted_pane_disposition(
    eligible: bool,
    busy: bool,
) -> UnwantedPaneDisposition {
    if !eligible {
        UnwantedPaneDisposition::Leave
    } else if busy {
        UnwantedPaneDisposition::Stash
    } else {
        UnwantedPaneDisposition::BreakOut
    }
}

/// Check if a tmux pane is running an active agent-doc or claude session.
fn is_pane_busy(tmux: &Tmux, pane_id: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{pane_pid}"])
        .output();
    let pid_str = match output {
        Ok(ref o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return false,
    };
    if pid_str.is_empty() {
        return false;
    }
    if pid_is_agent_session(&pid_str) {
        return true;
    }
    // Check child processes (pane PID is usually a shell)
    if let Ok(children) = std::process::Command::new("pgrep")
        .args(["-P", &pid_str])
        .output()
    {
        for child_pid in String::from_utf8_lossy(&children.stdout).lines() {
            let child_pid = child_pid.trim();
            if !child_pid.is_empty() && pid_is_agent_session(child_pid) {
                return true;
            }
        }
    }
    false
}

fn pid_is_agent_session(pid: &str) -> bool {
    let output = match std::process::Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let cmdline = String::from_utf8_lossy(&output.stdout);
    cmdline.contains("agent-doc") || cmdline.contains("claude")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tmux_router::{IsolatedTmux, Registry as SessionRegistry, RegistryEntry as SessionEntry};

    /// `#run3rdpaneswitch`, as a pure rule: BUSY changes where a pane goes, never
    /// whether it leaves. The regression was the opposite — busy meant "skip",
    /// so the outgoing session stayed visible and the incoming document's pane
    /// joined beside it.
    #[test]
    fn a_busy_unwanted_pane_is_stashed_never_left_visible() {
        assert_eq!(
            unwanted_pane_disposition(true, true),
            UnwantedPaneDisposition::Stash,
            "a busy pane must still leave the visible projection"
        );
        assert_ne!(
            unwanted_pane_disposition(true, true),
            UnwantedPaneDisposition::Leave,
            "leaving a busy pane visible is the third-pane defect"
        );
        assert_eq!(
            unwanted_pane_disposition(true, false),
            UnwantedPaneDisposition::BreakOut
        );
        // Ineligible panes are untouched regardless of what they are running:
        // wanted panes, unmanaged shells, and the last pane in a window.
        for busy in [false, true] {
            assert_eq!(
                unwanted_pane_disposition(false, busy),
                UnwantedPaneDisposition::Leave
            );
        }
    }

    fn write_doc(dir: &Path, name: &str, session: &str) -> std::path::PathBuf {
        let path = dir.join("tasks").join(name);
        std::fs::write(
            &path,
            format!("---\nagent_doc_session: {session}\n---\n\n# {name}\n"),
        )
        .unwrap();
        path
    }

    #[allow(clippy::too_many_arguments)]
    fn register(dir: &Path, session: &str, pane: &str, file: &str, window: &str) {
        let mut reg = agent_doc_session_registry_io::load_in(dir).unwrap_or_default();
        reg.insert(
            session.to_string(),
            SessionEntry {
                pane: pane.to_string(),
                pid: std::process::id(),
                cwd: dir.to_string_lossy().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: session.to_string(),
                file: file.to_string(),
                window: window.to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(dir, &reg).unwrap();
    }

    /// The operator's report, staged end to end: a document is running a turn,
    /// the editor switches to two other documents, and `layout` mirrors the new
    /// two-column split. The outgoing busy pane must leave the window — it used
    /// to stay, making three panes for a two-column editor — while staying alive
    /// in a `stash` window so a later sync can bring it back.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn switching_documents_while_the_outgoing_turn_runs_stays_two_panes() {
        let _env_guard = crate::test_support::env_lock();
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let previous = write_doc(root, "previous.md", "sess-previous");
        let first = write_doc(root, "first.md", "sess-first");
        let second = write_doc(root, "second.md", "sess-second");

        let iso = IsolatedTmux::new("layout-switch-while-busy");
        let busy_pane = iso.new_session("test", root).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let pane_first = iso.split_window(&busy_pane, root, "-dh").unwrap();
        let pane_second = iso.split_window(&busy_pane, root, "-dh").unwrap();
        let target_window = iso.pane_window(&busy_pane).unwrap();

        let _reg_seed: SessionRegistry = SessionRegistry::new();
        for (session, pane, file) in [
            ("sess-previous", &busy_pane, "tasks/previous.md"),
            ("sess-first", &pane_first, "tasks/first.md"),
            ("sess-second", &pane_second, "tasks/second.md"),
        ] {
            register(root, session, pane, file, &target_window);
        }

        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(root).unwrap();
        let result = run_with_tmux_and_busy_probe(
            &[first.as_path(), second.as_path()],
            Split::Horizontal,
            None,
            Some(&target_window),
            &iso,
            // The outgoing document's turn is still running.
            &|_tmux, pane_id| pane_id == busy_pane,
        );
        std::env::set_current_dir(cwd).unwrap();
        result.expect("layout should mirror the two-column editor split");

        let visible = iso.list_panes_ordered(&target_window).unwrap();
        assert!(
            !visible.contains(&busy_pane),
            "the outgoing busy pane must leave the two-column projection, got {visible:?}"
        );
        assert!(
            visible.contains(&pane_first) && visible.contains(&pane_second),
            "both requested documents must be visible, got {visible:?}"
        );
        assert_eq!(
            visible.len(),
            2,
            "a two-document editor projection must not grow a third pane: {visible:?}"
        );
        assert!(
            iso.pane_alive(&busy_pane),
            "moving the pane must not kill the running turn"
        );
        let stashed_window = iso.pane_window(&busy_pane).unwrap();
        assert_ne!(stashed_window, target_window);
        let name = iso
            .raw_cmd(&[
                "display-message",
                "-p",
                "-t",
                &stashed_window,
                "#{window_name}",
            ])
            .unwrap_or_default();
        assert!(
            name.trim() == "stash",
            "the busy pane must land in the tracked stash window so sync can rescue it, got {name:?}"
        );
        let _ = previous;
    }
}

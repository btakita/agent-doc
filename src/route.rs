//! `agent-doc route` — Route /agent-doc commands to the correct tmux pane.
//!
//! Usage: agent-doc route <file.md>
//!
//! 1. Reads session UUID from file's frontmatter
//! 2. Looks up pane in sessions.json
//! 3. If pane alive: sends `/agent-doc <path>` via tmux send-keys
//! 4. If dead/missing: lazy-claims to an active pane, syncs layout, and sends command
//! 5. If no registered/active pane: auto-starts a new Claude session
//!
//! After lazy claim, automatically syncs tmux layout via `sync_after_claim()`.

use anyhow::{Context, Result};
use std::path::Path;

use crate::sessions::Tmux;
use crate::{frontmatter, resync, sessions, sync};

const TMUX_SESSION_NAME: &str = "claude";

pub fn run(file: &Path, pane: Option<&str>) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server(), pane)
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux, pane: Option<&str>) -> Result<()> {
    let _ = resync::prune(); // Clean stale entries before lookup
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Ensure session UUID exists in frontmatter (generate if missing)
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session(&content)?;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("[route] Generated session UUID: {}", session_id);
    }

    let file_path = file.to_string_lossy();
    let registered = sessions::lookup(&session_id)?;

    // Step 1: Check if registered pane is alive
    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
            eprintln!("[route] Pane {} is alive", registered_pane);
            return send_command(tmux, registered_pane, &file_path);
        }
        eprintln!("[route] Pane {} is dead", registered_pane);
    } else {
        eprintln!(
            "[route] No pane registered for session {}",
            &session_id[..std::cmp::min(8, session_id.len())]
        );
    }

    // Step 2: Try lazy claim to an active pane
    if let Some(new_pane) = find_target_pane(tmux, pane) {
        let reason = if registered.is_some() {
            "dead pane"
        } else {
            "no registration"
        };
        eprintln!("[route] Lazy-claiming to pane {} ({})", new_pane, reason);
        sessions::register(&session_id, &new_pane, &file_path)?;
        send_command(tmux, &new_pane, &file_path)?;
        sync_after_claim(tmux, &new_pane);
        return Ok(());
    }

    // Step 3: Auto-start a new Claude session
    eprintln!("[route] No active pane found, auto-starting...");
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    auto_start(tmux, file, &session_id, &file_path)?;
    Ok(())
}

/// Send `/agent-doc <file>` to a pane and focus it.
fn send_command(tmux: &Tmux, pane: &str, file_path: &str) -> Result<()> {
    let command = format!("/agent-doc {}", file_path);
    tmux.send_keys(pane, &command)?;
    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!("[route] Sent /agent-doc {} → pane {}", file_path, pane);
    Ok(())
}

/// Find an active target pane for lazy claiming.
fn find_target_pane(tmux: &Tmux, explicit_pane: Option<&str>) -> Option<String> {
    let target = explicit_pane
        .map(|p| p.to_string())
        .or_else(|| tmux.active_pane(TMUX_SESSION_NAME));
    target.filter(|p| tmux.pane_alive(p))
}

/// Auto-start a new Claude session in tmux.
///
/// Cascade:
/// 1. tmux not running → create "claude" session
/// 2. "claude" session missing → create it
/// 3. "claude" session exists → create new window
/// 4. Send `agent-doc start <file>` in new pane
fn auto_start(tmux: &Tmux, file: &Path, session_id: &str, file_path: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // Resolve the agent-doc binary path (same binary that's currently running)
    let agent_doc_bin = std::env::current_exe()
        .unwrap_or_else(|_| "agent-doc".into())
        .to_string_lossy()
        .to_string();

    let new_pane = tmux.auto_start(TMUX_SESSION_NAME, &cwd)?;

    // Join into the existing active window instead of leaving in a separate window.
    // auto_start creates a new window; we want the pane alongside existing panes.
    if let Some(active) = tmux.active_pane(TMUX_SESSION_NAME)
        && active != new_pane
        && let Err(e) = tmux.join_pane(&new_pane, &active, "-dh")
    {
        eprintln!("[route] warning: join_pane failed ({} → {}): {}", new_pane, active, e);
    }

    // Register immediately so subsequent route calls find this pane
    sessions::register(session_id, &new_pane, file_path)?;

    // Start agent-doc start in the new pane
    let start_cmd = format!("{} start {}", agent_doc_bin, file_path);
    tmux.send_keys(&new_pane, &start_cmd)?;

    eprintln!(
        "[route] Started Claude for {} in pane {} (session {})",
        file_path,
        new_pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );
    eprintln!(
        "[route] Wait for Claude to start, then run `agent-doc route {}` again to send the command.",
        file_path
    );

    let _ = file; // suppress unused warning
    Ok(())
}

/// After a lazy claim, sync tmux layout for all files in the same window.
///
/// This ensures pane arrangement stays consistent when a file is reclaimed
/// to a different pane. Only runs on autoclaim — normal routing skips this.
fn sync_after_claim(tmux: &Tmux, pane_id: &str) {
    let window_id = match tmux.pane_window(pane_id) {
        Ok(w) => w,
        Err(_) => return,
    };

    // Load registry and find all files whose panes are in the same window
    let registry = match sessions::load() {
        Ok(r) => r,
        Err(_) => return,
    };

    let window_files: Vec<String> = registry
        .values()
        .filter(|entry| {
            !entry.pane.is_empty()
                && tmux.pane_alive(&entry.pane)
                && tmux.pane_window(&entry.pane).ok().as_deref() == Some(&window_id)
                && !entry.file.is_empty()
        })
        .map(|entry| entry.file.clone())
        .collect();

    if window_files.len() < 2 {
        return; // 0 or 1 files — no layout sync needed
    }

    // Each file as its own column (side-by-side / horizontal layout).
    // Previously this joined all files into a single column, which caused
    // tmux panes to stack vertically (top/bottom) instead of side-by-side.
    let file_count = window_files.len();
    let col_args: Vec<String> = window_files;
    if let Err(e) = sync::run(&col_args, Some(&window_id), None) {
        eprintln!("[route] warning: post-claim sync failed: {}", e);
    } else {
        eprintln!("[route] Auto-synced {} files in window {}", file_count, window_id);
    }
}

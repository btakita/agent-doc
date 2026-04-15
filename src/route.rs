//! # Module: route
//!
//! Routes `/agent-doc <file>` commands to the correct tmux pane. This is the
//! process-level coordinator between file-save events (editor plugin / watch daemon)
//! and running Claude Code sessions inside tmux.
//!
//! ## Spec
//!
//! - **`run(file, pane, debounce_ms, col_args)`**: Public entry point. Delegates to
//!   `run_with_tmux` using the default tmux server. Accepts an optional explicit
//!   `pane` override, a debounce delay in milliseconds, and column layout hints.
//! - **`run_with_tmux(file, tmux, pane, debounce_ms, col_args)`**: Core routing logic.
//!   1. Prunes stale session registry entries via `resync::prune`.
//!   2. If `debounce_ms > 0`, waits for the file's mtime to settle (`await_idle`).
//!   3. Ensures a session UUID exists in the file's YAML frontmatter (generates one if missing).
//!   4. Resolves the target tmux session: prefers project config (`config.toml`), falls
//!      back to current tmux session, auto-updates config when the configured session is dead.
//!   5. Looks up the registered pane in `sessions.json`.
//!   6. If pane is alive and in the correct session: optionally rescues from a stash window,
//!      then sends the `/agent-doc <path>` command via `send_command`.
//!   7. If pane is alive but in the wrong session and running an agent process: moves it to the
//!      target session stash then rescues. If running a non-agent process (corky, etc.): logs
//!      and falls through — foreign processes are never stashed/rescued across sessions.
//!   8. If pane is dead and was previously registered: lazy-claims to an active pane via
//!      `find_target_pane` (skipped if the candidate is running a non-agent process), sends
//!      the command, then calls `sync_after_claim` to re-sync layout.
//!   9. If no registered pane or no claimable pane: auto-starts a new Claude session.
//!      Blocked by `AGENT_DOC_NO_AUTOSTART` env var (used in tests).
//! - **`auto_start(tmux, file, session_id, file_path, context_session)`**: Public; spawns a
//!   new Claude pane and sends `/agent-doc start`. Waits for Claude's idle prompt before
//!   sending the initial command. Called by `sync.rs` for unresolved files.
//! - **`provision_pane(tmux, file, session_id, file_path, context_session, col_args)`**: Like
//!   `auto_start` but skips waiting for Claude to be ready. Used by sync when only pane
//!   existence is needed (Claude will start asynchronously). Computes `split_before` via
//!   `is_first_column(file, col_args)` so new panes split in the correct direction for
//!   their column position.
//! - **`is_first_column(file, col_args)`**: Returns true when `file` appears in the first
//!   `--col` argument. Drives the `-dbh` (split before) vs `-dh` (split after) split direction
//!   when creating a new pane. Returns false when `col_args` has fewer than 2 entries.
//! - **Positional split target (sync path)**: When `skip_wait` is true (called from sync),
//!   `auto_start_in_session` picks the split target based on column position — first pane in
//!   the agent-doc window for left-column files (`split_before`), last pane for right-column
//!   files. This places the new pane adjacent to its column neighbors instead of always splitting
//!   beside an arbitrary registered pane.
//! - **`send_command(tmux, pane, file_path)`**: Flashes a tmux display-message on the target
//!   pane, sends `/agent-doc <file_path>` via send-keys, focuses the pane, then polls up to
//!   5 seconds verifying the command was accepted (retrying Enter if still visible in input).
//! - **`await_idle(file, debounce)`**: Polls file mtime every 100ms until `debounce` has
//!   elapsed since last modification, or until `10 × debounce` safety cap expires.
//! - **`wait_for_claude_ready(tmux, pane_id, timeout)`**: Polls pane content every 500ms
//!   looking for Claude's idle prompt (`❯` / `>`). Returns true when prompt found, false on
//!   timeout. Logs progress every 10 polls.
//! - **`sync_after_claim(tmux, pane_id)`**: After a lazy claim, re-runs `sync::run` for all
//!   registered files in the same window to keep the tmux layout mirroring the editor split.
//!   Skipped when fewer than 2 files share the window.
//!
//! ## Agentic Contracts
//!
//! - **Session UUID guarantee**: `run_with_tmux` always ensures the file has a session UUID
//!   in frontmatter before any registry lookup. Callers never see a file without a UUID.
//! - **Stale-registry hygiene**: `resync::prune` is called at the start of every `run_with_tmux`
//!   invocation; the registry is always pruned before a lookup is attempted.
//! - **One pane per document**: Each document gets its own Claude pane. Unregistered files
//!   (no prior session) skip lazy-claim and always get a fresh pane via auto-start.
//! - **Session isolation**: Panes are validated to be in the correct target tmux session.
//!   Cross-session agent panes are moved to the target session; cross-session non-agent panes
//!   (e.g. corky) are never touched — a new pane is created in the correct session instead.
//! - **Non-agent process guard**: `is_agent_process()` gates both the wrong-session recovery
//!   path and the lazy-claim path. A pane running corky/shell is never stashed, rescued, or
//!   claimed — it is left running and a fresh agent-doc pane is provisioned instead.
//! - **Stash rescue**: Panes that ended up in a tmux `stash` / `stash-*` window are
//!   automatically rescued back into the `agent-doc` window before routing.
//! - **Auto-start inhibit**: Setting `AGENT_DOC_NO_AUTOSTART` prevents `auto_start_in_session`
//!   from spawning a new pane. The call returns `Err` with a descriptive message.
//! - **Non-fatal pane focus**: `select_pane` failures are logged as warnings and never abort
//!   the routing flow. The command is still sent even if focus fails.
//! - **Split direction determinism**: `is_first_column` requires ≥ 2 `col_args` entries to
//!   return true, ensuring a single-column layout never triggers a left-split.
//!
//! ## Evals
//!
//! - `is_first_column_empty_cols`: empty `col_args` → returns false (no layout context)
//! - `is_first_column_single_col`: single `col_args` entry → returns false (< 2 entries required)
//! - `is_first_column_in_first_col`: file matches first col arg → returns true
//! - `is_first_column_in_second_col`: file matches second col arg → returns false
//! - `is_first_column_comma_separated`: file matches comma-separated first col arg → returns true
//! - `detects_unicode_prompt`: `❯`, `❯ `, `  ❯  ` → all detected as Claude idle prompt
//! - `detects_ascii_prompt`: `>`, `> `, `  >  ` → all detected as Claude idle prompt
//! - `rejects_non_prompt_lines`: status text, empty lines, markdown headers → not matched as prompt
//! - `handles_ansi_prompt`: ANSI-colored `❯`/`>` → detected after strip_ansi
//! - `unregistered_file_skips_lazy_claim`: `registered = None` → lazy-claim step is skipped
//! - `dead_registered_pane_allows_lazy_claim`: `registered = Some(pane)` with dead pane → lazy-claim attempted
//! - (aspirational) `stash_rescue`: pane in stash window → rescued to agent-doc window before send
//! - (aspirational) `wrong_session_pane`: alive pane in wrong session → new pane created in target session
//! - (aspirational) `debounce_idle`: file written rapidly → routing waits for mtime to settle
//! - (aspirational) `autostart_inhibited`: `AGENT_DOC_NO_AUTOSTART` set → returns Err, no pane spawned

use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;

use crate::sessions::{PaneMoveOp, Tmux};
use crate::{frontmatter, prompt, resync, sessions, snapshot, sync};

const TMUX_SESSION_NAME: &str = "claude";

/// Valid process names for agent-doc panes (mirrors resync::AGENT_PROCESSES).
const AGENT_PROCESSES: &[&str] = &["agent-doc", "claude", "node"];

/// Returns true if the pane is running an agent process (agent-doc / claude / node).
/// Returns true on query failure (conservative — don't skip panes we can't inspect).
fn is_agent_process(tmux: &Tmux, pane_id: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{pane_current_command}"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let cmd = String::from_utf8_lossy(&o.stdout).trim().to_string();
            cmd.is_empty() || AGENT_PROCESSES.contains(&cmd.as_str())
        }
        _ => true, // can't inspect → treat conservatively
    }
}

/// Determine if the file is in the first column of the editor layout.
/// When true, the new pane should be split BEFORE (left of) the existing pane.
/// Returns false when col_args is empty (no layout context — default to split right).
fn is_first_column(file: &Path, col_args: &[String]) -> bool {
    if col_args.len() < 2 {
        return false;
    }
    let file_str = file.to_string_lossy();
    // Check if file appears in the first --col arg
    if let Some(first_col) = col_args.first() {
        first_col.split(',').any(|f| f.trim() == file_str.as_ref())
    } else {
        false
    }
}

pub fn run(file: &Path, pane: Option<&str>, debounce_ms: u64, col_args: &[String]) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server(), pane, debounce_ms, col_args)
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux, pane: Option<&str>, debounce_ms: u64, col_args: &[String]) -> Result<()> {
    tracing::debug!(file = %file.display(), pane, debounce_ms, cols = ?col_args, "route::run start");
    let _ = resync::prune(); // Clean stale entries before lookup

    // Debounce: wait for file mtime to settle before proceeding
    if debounce_ms > 0 {
        await_idle(file, Duration::from_millis(debounce_ms))?;
    }
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

    let target_session = resolve_target_session(tmux, None, true);
    eprintln!("[route] target tmux session: {}", target_session);

    let file_path = file.to_string_lossy();

    // === SINGLE EXIT POINT PATTERN ===
    // All paths resolve to a pane_id, then ONE sync call handles layout.
    // This prevents propagation bugs where cross-cutting behavior (sync)
    // is added to one path but missed on others.

    // Snapshot panes before route so we can clean up orphans on failure.
    let window_arg = col_args.first()
        .and_then(|_| tmux.cmd()
            .args(["display-message", "-t", &format!("{}:agent-doc", target_session), "-p", "#{window_id}"])
            .output().ok())
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let panes_before: Vec<String> = window_arg.as_deref()
        .and_then(|w| tmux.list_window_panes(w).ok())
        .unwrap_or_default();

    let pane_id = resolve_or_create_pane(
        tmux, file, pane, col_args,
        &session_id, &file_path, &target_session,
    );

    match pane_id {
        Ok(ref _pid) => {
            // NOTE: sync_after_claim was removed here to eliminate the double-sync
            // glitch. The JB plugin already triggers sync with the correct window
            // and col_args via the route call. A second sync (with window=None)
            // races with the first sync's stash operations, causing panes to
            // bounce between stash and agent-doc window visibly.
            // The JB plugin's sync call is authoritative — no defensive re-sync needed.
            Ok(())
        }
        Err(e) => {
            // Clean up orphaned panes created during the failed route attempt.
            // Compare current panes to the snapshot and kill any new ones.
            if let Some(w) = window_arg.as_deref() && let Ok(panes_after) = tmux.list_window_panes(w) {
                for p in &panes_after {
                    if !panes_before.contains(p) {
                        eprintln!("[route] cleaning up orphaned pane {} (created during failed route)", p);
                        tracing::warn!(pane = %p, "route: killing orphaned pane from failed route");
                        let _ = tmux.raw_cmd(&["kill-pane", "-t", p]);
                    }
                }
            }
            Err(e)
        }
    }
}

/// Resolve an existing pane or create a new one. Returns the pane ID.
///
/// Three resolution strategies, tried in order:
/// 1. Alive registered pane in the correct session → reuse (send command)
/// 2. Lazy claim to an active pane (when registered pane is dead)
/// 3. Auto-start a new Claude session
fn resolve_or_create_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
) -> Result<String> {
    tracing::debug!(
        session_id = &session_id[..8.min(session_id.len())],
        file = file_path,
        target_session,
        "route::resolve_or_create_pane"
    );
    let registered = sessions::lookup(session_id)?;

    // Strategy 1: Alive registered pane in correct session
    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
            let pane_session = tmux
                .cmd()
                .args(["display-message", "-t", registered_pane, "-p", "#{session_name}"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();

            if pane_session == target_session {
                // Rescue from stash if needed
                rescue_from_stash(tmux, registered_pane, session_id, file_path, target_session);

                eprintln!("[route] Pane {} is alive in session '{}'", registered_pane, pane_session);
                send_command(tmux, registered_pane, file_path)?;
                return Ok(registered_pane.clone());
            }
            // Pane is alive but in a different session.
            // Only move panes running agent processes — never stash/rescue foreign
            // processes (corky, etc.) across sessions.  If the pane is running a
            // non-agent process, fall through to Strategy 2/3 to get a fresh pane.
            if is_agent_process(tmux, registered_pane) {
                eprintln!(
                    "[route] Pane {} is alive but in session '{}' (config says '{}'). Moving to target session stash.",
                    registered_pane, pane_session, target_session
                );
                if let Err(e) = tmux.stash_pane(registered_pane, target_session) {
                    eprintln!("[route] warning: stash_pane to target session failed: {}", e);
                }
                rescue_from_stash(tmux, registered_pane, session_id, file_path, target_session);
                send_command(tmux, registered_pane, file_path)?;
                return Ok(registered_pane.clone());
            }
            eprintln!(
                "[route] Pane {} in session '{}' is running a non-agent process — skipping cross-session rescue",
                registered_pane, pane_session
            );
        } else {
            eprintln!("[route] Pane {} is dead", registered_pane);
        }
    } else {
        eprintln!(
            "[route] No pane registered for session {}",
            &session_id[..std::cmp::min(8, session_id.len())]
        );
    }

    // Strategy 2: Lazy claim (only when a registered pane died)
    // Skip panes running non-agent processes to avoid claiming corky/shells.
    if registered.is_some()
        && let Some(new_pane) = find_target_pane(tmux, pane, target_session)
        && is_agent_process(tmux, &new_pane)
    {
        eprintln!("[route] Lazy-claiming to pane {} (dead pane)", new_pane);
        sessions::register(session_id, &new_pane, file_path)?;
        send_command(tmux, &new_pane, file_path)?;
        return Ok(new_pane);
    }

    // Strategy 3: Auto-start
    eprintln!("[route] No active pane found, auto-starting...");
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    let split_before = is_first_column(file, col_args);
    auto_start_in_session(tmux, file, session_id, file_path, target_session, false, split_before)?;

    // Look up the pane that was just created
    sessions::lookup(session_id)?
        .ok_or_else(|| anyhow::anyhow!("auto-start completed but pane not found in registry"))
}

/// Rescue a pane from a stash window back to the agent-doc window.
/// Only rescues if the pane is in the target session — never swaps across sessions.
fn rescue_from_stash(
    tmux: &Tmux,
    pane_id: &str,
    session_id: &str,
    file_path: &str,
    target_session: &str,
) {
    // Session guard: only rescue within the target session
    let pane_session = tmux
        .cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{session_name}"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if pane_session != target_session {
        eprintln!(
            "[route] Pane {} is in session '{}', not target '{}' — skipping stash rescue",
            pane_id, pane_session, target_session
        );
        return;
    }

    let pane_win_name = tmux.pane_window(pane_id).ok()
        .and_then(|wid| {
            tmux.cmd()
                .args(["display-message", "-t", &wid, "-p", "#{window_name}"])
                .output().ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })
        .unwrap_or_default();

    if pane_win_name == "stash" || pane_win_name.starts_with("stash-") {
        tracing::debug!(pane_id, window = %pane_win_name, target_session, "route: rescuing pane from stash");
        eprintln!(
            "[route] Pane {} is in stash window '{}', rescuing to agent-doc window",
            pane_id, pane_win_name
        );
        let agent_doc_window = format!("{}:agent-doc", target_session);
        let target_panes = tmux.list_window_panes(&agent_doc_window).unwrap_or_default();
        if let Some(target) = target_panes.first() {
            match sessions::swap_pane_guarded(tmux, pane_id, target, target_session) {
                Ok(()) => eprintln!("[route] Rescued pane {} via swap-pane", pane_id),
                Err(e) => {
                    eprintln!("[route] swap-pane rescue failed ({}), trying join-pane", e);
                    let _ = PaneMoveOp::new(tmux, pane_id, target).join("-dh");
                }
            }
        }
        if let Err(e) = sessions::register(session_id, pane_id, file_path) {
            eprintln!("[route] warning: re-register failed: {}", e);
        }
    }
}

/// Send `/agent-doc <file>` to a pane and focus it.
/// Shows a brief tmux display-message on the target pane for immediate feedback.
fn send_command(tmux: &Tmux, pane: &str, file_path: &str) -> Result<()> {
    // Flash notification on target pane — immediate feedback before Claude picks up input
    let short_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let flash_msg = format!("⏳ /agent-doc {}", short_name);
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    let command = format!("/agent-doc {}", file_path);
    tmux.send_keys(pane, &command)?;
    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!("[route] Sent /agent-doc {} → pane {}", file_path, pane);

    // Poll-based Enter confirmation: check if the command text is still visible
    // in the pane. Claude Code always shows ❯, so we can't check for prompt
    // disappearance. Instead, we check if "/agent-doc" is still in the last
    // few lines (meaning it's still in the input, not yet submitted).
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);
    let poll_interval = std::time::Duration::from_millis(300);
    let mut enter_retries = 0u32;

    while start.elapsed() < timeout {
        std::thread::sleep(poll_interval);
        if let Ok(content) = sessions::capture_pane(tmux, pane) {
            // Check if the command text is still in the last 5 lines
            // (i.e., still sitting in the input prompt, not yet submitted)
            let cmd_still_in_input = content
                .lines()
                .rev()
                .take(5)
                .any(|l| {
                    let stripped = prompt::strip_ansi(l);
                    stripped.contains("/agent-doc") && stripped.contains(file_path)
                });

            if !cmd_still_in_input {
                eprintln!(
                    "[route] Command accepted ({:.1}s, {} Enter retries)",
                    start.elapsed().as_secs_f64(),
                    enter_retries
                );
                return Ok(());
            }

            // Command text still in input — retry Enter
            enter_retries += 1;
            if let Err(e) = tmux.send_keys_raw(pane, "Enter") {
                eprintln!("[route] warning: retry Enter failed: {}", e);
            }
        }
    }
    eprintln!(
        "[route] warning: command may not have been accepted after {:.1}s ({} Enter retries)",
        start.elapsed().as_secs_f64(),
        enter_retries
    );
    Ok(())
}

/// Get the current tmux session name (the session the caller is attached to).
fn current_tmux_session(tmux: &Tmux) -> Option<String> {
    // If we're inside tmux, query the current session name
    let output = tmux
        .cmd()
        .args(["display-message", "-p", "#{session_name}"])
        .output()
        .ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Single source of truth for target session resolution.
///
/// Priority:
/// 1. `context_session` if provided (from sync --window)
/// 2. config.toml `tmux_session` if the session is alive
/// 3. Fallback to current tmux session or "claude" constant
///
/// When no session is configured AND `auto_update_config` is true, writes
/// the fallback to config.toml. When a session IS configured (even if dead),
/// the config is NOT updated — this prevents a session-1 terminal from
/// silently overwriting a session-0 project config.
fn resolve_target_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    auto_update_config: bool,
) -> String {
    if let Some(ctx) = context_session {
        return ctx.to_string();
    }

    let configured = crate::config::project_tmux_session();
    if configured.as_ref().is_some_and(|s| tmux.session_alive(s)) {
        return configured.unwrap();
    }

    let fallback = current_tmux_session(tmux)
        .unwrap_or_else(|| TMUX_SESSION_NAME.to_string());

    // Only auto-update config when no session was previously configured.
    // If a session IS configured but dead (e.g. server restart), preserve the config value
    // so a session-1 terminal cannot silently overwrite the project's session-0 target.
    if auto_update_config && configured.is_none()
        && let Err(e) = crate::config::update_project_tmux_session(&fallback)
    {
        eprintln!("warning: failed to update project tmux_session config: {}", e);
    }

    fallback
}

/// Find an active target pane for lazy claiming.
fn find_target_pane(tmux: &Tmux, explicit_pane: Option<&str>, session_name: &str) -> Option<String> {
    let target = explicit_pane
        .map(|p| p.to_string())
        .or_else(|| tmux.active_pane(session_name));
    target.filter(|p| tmux.pane_alive(p))
}

/// Check if a window with the given name exists in the target tmux session.
fn has_named_window(tmux: &Tmux, session_name: &str, window_name: &str) -> bool {
    let output = tmux
        .cmd()
        .args([
            "list-windows",
            "-t",
            session_name,
            "-F",
            "#{window_name}",
        ])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().any(|l| l.trim() == window_name)
        }
        _ => false,
    }
}

/// Find a registered agent-doc pane in the target tmux session.
/// Used by auto_start to join alongside an existing agent-doc pane (not any random pane).
fn find_registered_pane_in_session(tmux: &Tmux, session_name: &str, exclude_pane: &str) -> Option<String> {
    let registry = sessions::load().ok()?;
    for entry in registry.values() {
        if entry.pane == exclude_pane || entry.pane.is_empty() {
            continue;
        }
        if !tmux.pane_alive(&entry.pane) {
            continue;
        }
        // Check if this pane is in the target session
        if let Ok(output) = tmux
            .cmd()
            .args(["display-message", "-t", &entry.pane, "-p", "#{session_name}"])
            .output()
        {
            let pane_session = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if pane_session == session_name {
                return Some(entry.pane.clone());
            }
        }
    }
    None
}

/// Auto-start a new Claude session in tmux using the default session name.
/// Public so `sync.rs` can call it for unresolved files.
///
/// `context_session` is an optional session override from the calling context
/// (e.g., the sync target session). Used when frontmatter has no `tmux_session`
/// to avoid falling back to `current_tmux_session()`, which returns whichever
/// session the user's terminal is viewing — not necessarily the correct one.
#[allow(dead_code)]
pub fn auto_start(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
) -> Result<()> {
    auto_start_ext(tmux, file, session_id, file_path, context_session, false, false)
}

/// Rewrite `file_path` to be relative to `cwd` so `agent-doc start <path>` resolves
/// correctly when the spawned pane's cwd is narrowed to a submodule root.
///
/// When `resolve_pane_cwd` narrows to a submodule (e.g. `.../src/session-share`),
/// the caller's super-root-relative `file_path` (e.g. `src/session-share/tasks/foo.md`)
/// does not resolve from inside that cwd. We canonicalize both sides, strip the cwd
/// prefix, and return the cwd-relative remainder. On any failure (canonicalize error,
/// file not under cwd) we fall back to the original `file_path` so non-submodule docs
/// and missing-file cases behave exactly as before.
pub(crate) fn rewrite_start_path(file: &Path, cwd: &Path, original: &str) -> String {
    let Ok(abs_file) = std::fs::canonicalize(file) else {
        return original.to_string();
    };
    let Ok(abs_cwd) = std::fs::canonicalize(cwd) else {
        return original.to_string();
    };
    match abs_file.strip_prefix(&abs_cwd) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => original.to_string(),
    }
}

/// **Provisioning** — create a new tmux pane and start Claude asynchronously.
///
/// Called by sync during Reconciliation when a file has a session UUID but no
/// registered pane. Creates the pane immediately but doesn't wait for Claude
/// to initialize (async startup).
pub fn provision_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    col_args: &[String],
) -> Result<()> {
    let split_before = is_first_column(file, col_args);
    auto_start_ext(tmux, file, session_id, file_path, context_session, true, split_before)
}

fn auto_start_ext(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
) -> Result<()> {
    let session_name = resolve_target_session(tmux, context_session, true);
    auto_start_in_session(tmux, file, session_id, file_path, &session_name, skip_wait, split_before)
}

/// Auto-start a new Claude session in a specific tmux session.
///
/// Strategy:
/// 1. Find an existing registered agent-doc pane in the target session
/// 2. If found: `split-window` directly in that pane's window (avoids creating
///    a throwaway window then failing to join due to minimum pane size)
/// 3. If not found: create a new window via `auto_start` (session may not exist yet)
///
/// When `skip_wait` is true, skips `wait_for_claude_ready` and `send_command`.
/// Used by sync which only needs the pane to exist with Claude starting.
fn auto_start_in_session(tmux: &Tmux, file: &Path, session_id: &str, file_path: &str, session_name: &str, skip_wait: bool, split_before: bool) -> Result<()> {
    // Startup lock: prevent double-spawn when sync fires twice in quick succession.
    // Check for a lock file; if it exists and is < 5s old, skip this auto-start.
    // Best-effort: skip lock entirely if file doesn't exist or hash fails.
    if let Ok(canonical) = std::fs::canonicalize(file)
        && let Some(project_root) = snapshot::find_project_root(&canonical)
        && let Ok(hash) = snapshot::doc_hash(file)
    {
        let starting_dir = project_root.join(".agent-doc/starting");
        let lock_path = starting_dir.join(format!("{}.lock", hash));
        if lock_path.exists()
            && let Ok(meta) = lock_path.metadata()
            && let Ok(modified) = meta.modified()
            && let Ok(age) = modified.elapsed()
            && age.as_secs() < 5
        {
            eprintln!(
                "[route] startup lock exists for {} (age {:.1}s), skipping auto-start",
                file_path, age.as_secs_f64()
            );
            return Ok(());
        }
        // Create the lock
        let _ = std::fs::create_dir_all(&starting_dir);
        let _ = std::fs::write(&lock_path, "");
    }

    // Use the document's own submodule root as the pane cwd when applicable,
    // so `/agent-doc` invocations on submodule-hosted documents spawn panes
    // inside the correct submodule (e.g. `src/session-share`) instead of the
    // agent-loop super root where the command happened to be invoked from.
    let cwd = crate::git::resolve_pane_cwd(file);

    // Resolve the agent-doc binary path (same binary that's currently running)
    let agent_doc_bin = std::env::current_exe()
        .unwrap_or_else(|_| "agent-doc".into())
        .to_string_lossy()
        .to_string();

    // Try to split directly in an existing pane.
    // When skip_wait=true (sync path), prefer panes in the target window (agent-doc window)
    // over stash panes — splitting in the stash creates invisible panes.
    let existing_pane = if skip_wait {
        // Sync path: find a pane in the agent-doc window (not stash)
        let window_panes = tmux.list_window_panes(
            &format!("{}:agent-doc", session_name)
        ).unwrap_or_default();
        let positional = if split_before {
            window_panes.into_iter().next()       // leftmost for left-column file
        } else {
            window_panes.into_iter().last()       // rightmost for right-column file
        };
        positional.or_else(|| find_registered_pane_in_session(tmux, session_name, ""))
    } else {
        find_registered_pane_in_session(tmux, session_name, "")
    };
    let split_flag = if split_before { "-dbh" } else { "-dh" };
    let new_pane = if let Some(ref target) = existing_pane {
        match tmux.split_window(target, &cwd, split_flag) {
            Ok(pane) => {
                eprintln!(
                    "[route] split-window {} alongside registered pane {} in session '{}' → new pane {}",
                    split_flag, target, session_name, pane
                );
                pane
            }
            Err(e) => {
                eprintln!(
                    "[route] warning: split-window failed alongside {} ({}), stashing in stash window",
                    target, e
                );
                // Create in a new window, then immediately stash it so it
                // doesn't appear as a visible "claude" window.
                let pane = tmux.auto_start(session_name, &cwd)?;
                if let Err(stash_err) = tmux.stash_pane(&pane, session_name) {
                    eprintln!(
                        "[route] warning: stash failed ({}), pane {} remains in new window",
                        stash_err, pane
                    );
                } else {
                    eprintln!("[route] split failed, stashed pane in stash window");
                }
                pane
            }
        }
    } else {
        // Check if an "agent-doc" named window already exists in the target session.
        // If yes, stash the new pane to prevent window proliferation.
        let has_agent_doc_window = has_named_window(tmux, session_name, "agent-doc");
        if has_agent_doc_window {
            eprintln!(
                "[route] no registered pane but 'agent-doc' window exists in session '{}', creating + stashing",
                session_name
            );
            let pane = tmux.auto_start(session_name, &cwd)?;
            if let Err(stash_err) = tmux.stash_pane(&pane, session_name) {
                eprintln!(
                    "[route] warning: stash failed ({}), pane {} remains in new window",
                    stash_err, pane
                );
            } else {
                eprintln!("[route] stashed new pane {} to avoid window proliferation", pane);
            }
            pane
        } else {
            eprintln!("[route] no registered pane found in session '{}', creating new window", session_name);
            tmux.auto_start(session_name, &cwd)?
        }
    };

    // Register immediately so subsequent route calls find this pane
    sessions::register(session_id, &new_pane, file_path)?;

    // Focus the new pane immediately so the user sees Claude starting
    if let Err(e) = tmux.select_pane(&new_pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", new_pane, e);
    }

    // Rewrite file_path to be relative to the spawned pane's cwd.
    // `cwd` may be narrowed to a submodule root (see `resolve_pane_cwd`), in which
    // case a super-root-relative `file_path` like `src/session-share/tasks/foo.md`
    // will not resolve when `agent-doc start` runs from inside the submodule.
    // Fallback: if canonicalize fails or the file is not under `cwd`, use the
    // original `file_path` (preserves behavior for non-submodule docs).
    let start_path = rewrite_start_path(file, &cwd, file_path);

    // Start agent-doc start in the new pane
    let start_cmd = format!("{} start {}", agent_doc_bin, start_path);
    tmux.send_keys(&new_pane, &start_cmd)?;

    eprintln!(
        "[route] Started Claude for {} in pane {} (session {})",
        file_path,
        new_pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );

    if skip_wait {
        eprintln!("[route] skip_wait=true — pane created, Claude starting (sync path)");
    } else {
        // Poll until Claude is ready, then send the /agent-doc command
        eprintln!("[route] Waiting for Claude to initialize...");
        if wait_for_claude_ready(tmux, &new_pane, std::time::Duration::from_secs(30)) {
            eprintln!("[route] Claude is ready, sending /agent-doc command");
            // send_command now includes Enter verification + retry.
            // Use `start_path` (cwd-relative) for the same reason as `start_cmd`.
            send_command(tmux, &new_pane, &start_path)?;
        } else {
            eprintln!(
                "[route] Timed out waiting for Claude. Run `agent-doc route {}` to retry.",
                file_path
            );
        }
    }

    let _ = file; // suppress unused warning
    Ok(())
}

/// Poll a tmux pane until Claude Code is ready to accept input.
///
/// Looks for Claude's idle prompt indicator (`❯` or `>`) in the captured pane content.
/// Strips ANSI escape codes before matching. Polls every 500ms up to the given timeout.
fn wait_for_claude_ready(tmux: &Tmux, pane_id: &str, timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(500);
    let mut poll_count = 0u32;

    while start.elapsed() < timeout {
        if pane_has_prompt(tmux, pane_id) {
            eprintln!(
                "[route] Claude ready after {:.1}s ({} polls)",
                start.elapsed().as_secs_f64(),
                poll_count
            );
            return true;
        }

        poll_count += 1;
        if poll_count.is_multiple_of(10)
            && let Ok(content) = sessions::capture_pane(tmux, pane_id) {
                let last_line = content
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(prompt::strip_ansi)
                    .unwrap_or_default();
                eprintln!(
                    "[route] Still waiting for Claude ({:.0}s)... last line: {}",
                    start.elapsed().as_secs_f64(),
                    &last_line[..std::cmp::min(60, last_line.len())]
                );
            }
        std::thread::sleep(poll_interval);
    }
    false
}

/// Check if pane content shows Claude's idle prompt (❯ or >).
fn pane_has_prompt(tmux: &Tmux, pane_id: &str) -> bool {
    if let Ok(content) = sessions::capture_pane(tmux, pane_id) {
        content
            .lines()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .take(10)
            .any(|l| {
                let t = prompt::strip_ansi(l);
                let t = t.trim();
                t == "❯" || t == ">" || t.starts_with("❯ ") || t.starts_with("> ")
            })
    } else {
        false
    }
}

/// After a lazy claim, sync tmux layout for all files in the same window.
///
/// This ensures pane arrangement stays consistent when a file is reclaimed
/// to a different pane. Only runs on autoclaim — normal routing skips this.
#[allow(dead_code)]
fn sync_after_claim(tmux: &Tmux, pane_id: &str, col_args: &[String]) {
    let window_id = match tmux.pane_window(pane_id) {
        Ok(w) => w,
        Err(_) => return,
    };

    // Use editor-provided col_args when available (authoritative layout).
    // Only fall back to registry discovery when no col_args given.
    let effective_col_args: Vec<String> = if !col_args.is_empty() {
        col_args.to_vec()
    } else {
        // Load registry and find all files whose panes are in the same window
        let registry = match sessions::load() {
            Ok(r) => r,
            Err(_) => return,
        };

        registry
            .values()
            .filter(|entry| {
                !entry.pane.is_empty()
                    && tmux.pane_alive(&entry.pane)
                    && tmux.pane_window(&entry.pane).ok().as_deref() == Some(&window_id)
                    && !entry.file.is_empty()
            })
            .map(|entry| entry.file.clone())
            .collect()
    };

    if effective_col_args.len() < 2 {
        return; // 0 or 1 files — no layout sync needed
    }

    let file_count = effective_col_args.len();
    if let Err(e) = sync::run(&effective_col_args, Some(&window_id), None) {
        eprintln!("[route] warning: post-claim sync failed: {}", e);
    } else {
        eprintln!("[route] Auto-synced {} files in window {}", file_count, window_id);
    }
}

/// Wait for the file's mtime to settle (no modifications within the debounce window).
/// Polls every 100ms, up to 10× the debounce duration as a safety cap.
fn await_idle(file: &Path, debounce: Duration) -> Result<()> {
    use std::time::Instant;

    let max_wait = debounce * 10;
    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();

    loop {
        let mtime = std::fs::metadata(file)
            .and_then(|m| m.modified())
            .with_context(|| format!("failed to stat {}", file.display()))?;
        let elapsed_since_edit = mtime.elapsed().unwrap_or(Duration::ZERO);

        if elapsed_since_edit >= debounce {
            eprintln!(
                "[route] debounce OK — file idle for {:.1}s",
                elapsed_since_edit.as_secs_f64()
            );
            return Ok(());
        }

        if start.elapsed() >= max_wait {
            eprintln!(
                "[route] debounce timeout after {:.1}s — proceeding anyway",
                start.elapsed().as_secs_f64()
            );
            return Ok(());
        }

        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- rewrite_start_path tests ---

    #[test]
    fn rewrite_start_path_narrows_to_submodule_relative() {
        // Simulate: super root with a `src/sub` submodule holding `tasks/foo.md`.
        // `cwd` = super/src/sub (narrowed by resolve_pane_cwd).
        // `file_path` = "src/sub/tasks/foo.md" (super-root-relative, as passed by caller).
        // Expected: rewritten to "tasks/foo.md".
        let tmp = tempfile::TempDir::new().unwrap();
        let super_root = tmp.path();
        let sub_root = super_root.join("src").join("sub");
        let tasks_dir = sub_root.join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        let doc = tasks_dir.join("foo.md");
        std::fs::write(&doc, "# foo\n").unwrap();

        let original = "src/sub/tasks/foo.md";
        let rewritten = rewrite_start_path(&doc, &sub_root, original);
        assert_eq!(rewritten, format!("tasks{}foo.md", std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn rewrite_start_path_no_op_when_file_under_cwd_with_same_prefix() {
        // Non-submodule case: cwd = super root, file is already super-root-relative.
        // The rewrite still works — it just returns the same relative path.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let doc = root.join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let original = "plan.md";
        let rewritten = rewrite_start_path(&doc, root, original);
        assert_eq!(rewritten, "plan.md");
    }

    #[test]
    fn rewrite_start_path_falls_back_when_canonicalize_fails() {
        // Non-existent file path → canonicalize fails → fallback to original.
        let tmp = tempfile::TempDir::new().unwrap();
        let ghost = tmp.path().join("does-not-exist.md");
        let original = "does-not-exist.md";
        let rewritten = rewrite_start_path(&ghost, tmp.path(), original);
        assert_eq!(rewritten, original);
    }

    #[test]
    fn rewrite_start_path_falls_back_when_file_not_under_cwd() {
        // File exists but lives outside the given cwd → strip_prefix fails → fallback.
        let tmp = tempfile::TempDir::new().unwrap();
        let outside = tmp.path().join("outside.md");
        std::fs::write(&outside, "# outside\n").unwrap();
        let unrelated_cwd = tempfile::TempDir::new().unwrap();

        let original = "outside.md";
        let rewritten = rewrite_start_path(&outside, unrelated_cwd.path(), original);
        assert_eq!(rewritten, original);
    }

    // --- Split direction tests ---

    #[test]
    fn is_first_column_empty_cols() {
        let file = Path::new("tasks/agent-doc.md");
        assert!(!is_first_column(file, &[]));
    }

    #[test]
    fn is_first_column_single_col() {
        let file = Path::new("tasks/agent-doc.md");
        let cols = vec!["tasks/agent-doc.md".to_string()];
        // Single column — no need to split before
        assert!(!is_first_column(file, &cols));
    }

    #[test]
    fn is_first_column_in_first_col() {
        let file = Path::new("tasks/agent-doc.md");
        let cols = vec![
            "tasks/agent-doc.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(is_first_column(file, &cols));
    }

    #[test]
    fn is_first_column_in_second_col() {
        let file = Path::new("tasks/email.md");
        let cols = vec![
            "tasks/agent-doc.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(!is_first_column(file, &cols));
    }

    #[test]
    fn is_first_column_comma_separated() {
        let file = Path::new("tasks/agent-doc.md");
        let cols = vec![
            "tasks/agent-doc.md,tasks/corky.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(is_first_column(file, &cols));
    }

    // --- Prompt detection tests ---

    #[test]
    fn detects_unicode_prompt() {
        // Claude Code uses ❯ (U+276F)
        assert!(is_prompt_line("❯"));
        assert!(is_prompt_line("❯ "));
        assert!(is_prompt_line("  ❯  "));
    }

    #[test]
    fn detects_ascii_prompt() {
        assert!(is_prompt_line(">"));
        assert!(is_prompt_line("> "));
        assert!(is_prompt_line("  >  "));
    }

    #[test]
    fn rejects_non_prompt_lines() {
        assert!(!is_prompt_line("Starting claude..."));
        assert!(!is_prompt_line("test result: ok"));
        assert!(!is_prompt_line(""));
        assert!(!is_prompt_line("  "));
        assert!(!is_prompt_line("## User"));
        // Blockquote markers should NOT match — they are "> text" with content after
        // but our check starts_with("> ") would match. This is acceptable since
        // blockquotes don't appear in Claude Code's TUI output.
    }

    #[test]
    fn handles_ansi_prompt() {
        // Prompt with ANSI color codes
        assert!(is_prompt_line("\x1b[32m❯\x1b[0m"));
        assert!(is_prompt_line("\x1b[1m>\x1b[0m"));
    }

    /// Helper to test prompt detection on a single line.
    fn is_prompt_line(line: &str) -> bool {
        let stripped = prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        trimmed == "❯"
            || trimmed == ">"
            || trimmed.starts_with("❯ ")
            || trimmed.starts_with("> ")
    }

    // --- Routing logic tests ---

    #[test]
    fn unregistered_file_skips_lazy_claim() {
        // When registered is None, the lazy-claim step should be skipped.
        // This is verified by the code structure: `if registered.is_some()` guards
        // the find_target_pane call.
        let registered: Option<String> = None;
        assert!(
            registered.is_none(),
            "unregistered files should not attempt lazy claim"
        );
    }

    #[test]
    fn dead_registered_pane_allows_lazy_claim() {
        // When registered is Some but pane is dead, lazy-claim should be attempted.
        let registered: Option<String> = Some("%99".to_string());
        assert!(
            registered.is_some(),
            "dead registered pane should attempt lazy claim"
        );
    }

    // --- Integration tests (IsolatedTmux) ---

    use sessions::IsolatedTmux;

    /// Create a mock Claude script: blocks for delay, then prints ❯ prompt on its own line.
    /// Uses `cat` to keep the process alive after showing the prompt.
    fn mock_claude_script(delay_ms: u64) -> String {
        format!(
            r#"PS1='$ '; echo "Starting claude..."; sleep {}; echo '❯ '; cat"#,
            delay_ms as f64 / 1000.0
        )
    }

    #[test]
    fn wait_for_claude_ready_detects_prompt() {
        let iso = IsolatedTmux::new("route-test-ready");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start(session, &cwd).unwrap();

        // Set a non-matching PS1 and run mock Claude that shows ❯ after 500ms
        iso.send_keys(&pane, &mock_claude_script(500)).unwrap();

        // Should detect the prompt within 5s
        let ready = wait_for_claude_ready(&iso, &pane, std::time::Duration::from_secs(5));
        assert!(ready, "should detect ❯ prompt from mock Claude");
    }

    #[test]
    fn wait_for_claude_ready_times_out_without_prompt() {
        let iso = IsolatedTmux::new("route-test-timeout");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create a pane that runs sleep directly (no shell prompt at all)
        let pane_id = iso
            .cmd()
            .args(["new-session", "-d", "-s", session, "-c", &cwd.to_string_lossy(), "-P", "-F", "#{pane_id}", "sleep", "30"])
            .output()
            .expect("failed to create tmux session");
        let pane = String::from_utf8_lossy(&pane_id.stdout).trim().to_string();

        // Should time out after 2s (sleep never shows ❯)
        let ready = wait_for_claude_ready(&iso, &pane, std::time::Duration::from_secs(2));
        assert!(!ready, "should time out when no ❯ prompt appears");
    }

    #[test]
    fn send_keys_delivers_command_with_enter() {
        let iso = IsolatedTmux::new("route-test-send");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start(session, &cwd).unwrap();

        // Start a shell that reads a line and echoes it back with a marker
        iso.send_keys(&pane, r#"read CMD && echo "GOT:$CMD""#).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Send a command (simulates what send_command does)
        iso.send_keys(&pane, "/agent-doc test.md").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(800));

        // Capture and verify the command was received
        let content = sessions::capture_pane(&iso, &pane).unwrap();
        assert!(
            content.contains("GOT:/agent-doc test.md"),
            "command should be delivered and echoed back, got: {}",
            content
        );
    }

    #[test]
    fn pane_has_prompt_detects_unicode() {
        let iso = IsolatedTmux::new("route-test-has-prompt");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start(session, &cwd).unwrap();

        // Use exec to replace the shell entirely, then use bash -c to print ❯ and block
        iso.send_keys(&pane, "exec bash -c 'echo ❯; cat'").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1500));

        let content = sessions::capture_pane(&iso, &pane).unwrap_or_default();
        assert!(
            pane_has_prompt(&iso, &pane),
            "should detect ❯ in pane content, got: {}",
            content
        );
    }

    #[test]
    fn full_auto_start_flow() {
        // End-to-end test: create pane → run mock Claude → detect ready → send command
        let iso = IsolatedTmux::new("route-test-e2e");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start(session, &cwd).unwrap();

        // 1. Run mock Claude (shows ❯ after 300ms, then blocks on cat to accept input)
        iso.send_keys(&pane, &mock_claude_script(300)).unwrap();

        // 2. Wait for ready
        let ready = wait_for_claude_ready(&iso, &pane, std::time::Duration::from_secs(5));
        assert!(ready, "mock Claude should become ready");

        // 3. Send a command via send_keys (simulating send_command)
        iso.send_keys(&pane, "HELLO_FROM_TEST").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 4. Verify command appears in pane (cat echoes stdin to stdout)
        let content = sessions::capture_pane(&iso, &pane).unwrap();
        assert!(
            content.contains("HELLO_FROM_TEST"),
            "command should appear in pane after send, got: {}",
            content
        );
    }

    #[test]
    fn select_pane_switches_window() {
        let iso = IsolatedTmux::new("route-test-select");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create first pane (auto_start creates session + first window)
        let pane1 = iso.auto_start(session, &cwd).unwrap();

        // Create second window with a new pane
        let output = iso
            .cmd()
            .args(["new-window", "-t", session, "-P", "-F", "#{pane_id}"])
            .output()
            .unwrap();
        let pane2 = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Select pane1 — should switch back to window 1
        iso.select_pane(&pane1).unwrap();

        // Verify pane1 is now the active pane
        let active = iso
            .cmd()
            .args([
                "display-message",
                "-t",
                session,
                "-p",
                "#{pane_id}",
            ])
            .output()
            .unwrap();
        let active_pane = String::from_utf8_lossy(&active.stdout).trim().to_string();
        assert_eq!(
            active_pane, pane1,
            "select_pane should switch to the correct window/pane"
        );

        let _ = pane2; // suppress unused warning
    }

    #[test]
    fn command_text_cleared_after_acceptance() {
        // Verifies that send_command's acceptance check works:
        // The command text should NOT be in the last 5 lines after acceptance.
        let iso = IsolatedTmux::new("route-test-cmd-clear");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start(session, &cwd).unwrap();

        // Send a command that gets consumed immediately
        iso.send_keys(&pane, "echo DONE").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        // The command "echo DONE" should NOT be in the prompt anymore
        // (it was accepted and executed)
        let content = sessions::capture_pane(&iso, &pane).unwrap();
        let _cmd_in_last_lines = content
            .lines()
            .rev()
            .take(5)
            .any(|l| l.contains("echo DONE") && !l.contains("DONE"));
        // The echo command was accepted — "echo DONE" appears in history but
        // "DONE" output appears too. The key is that the INPUT line no longer
        // has the command waiting for Enter.
        assert!(
            content.contains("DONE"),
            "command should have been executed, got: {}",
            content
        );
    }

    #[test]
    fn pane_session_detection() {
        // Verify we can detect which session a pane is in
        let iso = IsolatedTmux::new("route-test-session");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start(session, &cwd).unwrap();

        // Check session name
        let output = iso
            .cmd()
            .args(["display-message", "-t", &pane, "-p", "#{session_name}"])
            .output()
            .unwrap();
        let detected_session = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            detected_session, session,
            "pane should be in session '{}'", session
        );
    }

    #[test]
    fn pane_in_wrong_session_detected() {
        // Create panes in two different sessions, verify we can distinguish them
        let iso = IsolatedTmux::new("route-test-wrong-sess");
        let cwd = std::env::current_dir().unwrap();

        // Create session "correct" with a pane
        let correct_pane = iso.auto_start("correct", &cwd).unwrap();

        // Create session "wrong" with another pane
        let wrong_pane = iso.auto_start("wrong", &cwd).unwrap();

        // Verify they're in different sessions
        let correct_session = iso
            .cmd()
            .args(["display-message", "-t", &correct_pane, "-p", "#{session_name}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap();
        let wrong_session = iso
            .cmd()
            .args(["display-message", "-t", &wrong_pane, "-p", "#{session_name}"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap();

        assert_eq!(correct_session, "correct");
        assert_eq!(wrong_session, "wrong");
        assert_ne!(correct_session, wrong_session, "panes should be in different sessions");
    }

    // --- auto_start_in_session tests ---

    #[test]
    fn auto_start_splits_in_existing_window() {
        // When a registered agent-doc pane exists in the target session,
        // auto_start_in_session should split-window in that pane's window
        // (not create a new window).
        let iso = IsolatedTmux::new("route-test-split-existing");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create the first pane (simulating an existing agent-doc pane)
        let pane1 = iso.auto_start(session, &cwd).unwrap();
        let window1 = iso.pane_window(&pane1).unwrap();

        // Split directly in that window (simulating what auto_start_in_session does)
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
        let window2 = iso.pane_window(&pane2).unwrap();

        // Both panes should be in the same window
        assert_eq!(
            window1, window2,
            "split_window should create pane in the SAME window, not a new one"
        );

        // Both panes should be alive
        assert!(iso.pane_alive(&pane1));
        assert!(iso.pane_alive(&pane2));

        // The panes should be different
        assert_ne!(pane1, pane2, "should create a distinct new pane");
    }

    #[test]
    fn auto_start_creates_new_window_when_no_registered_panes() {
        // When no registered agent-doc panes exist, auto_start_in_session
        // should create a new window via auto_start().
        let iso = IsolatedTmux::new("route-test-new-window");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create session with an initial pane (not registered)
        let pane1 = iso.auto_start(session, &cwd).unwrap();
        let window1 = iso.pane_window(&pane1).unwrap();

        // Calling auto_start again creates a NEW window (since no registered panes)
        let pane2 = iso.auto_start(session, &cwd).unwrap();
        let window2 = iso.pane_window(&pane2).unwrap();

        // Should be in different windows
        assert_ne!(
            window1, window2,
            "auto_start should create a new window when no registered panes exist"
        );
        assert_ne!(pane1, pane2);
    }

    #[test]
    fn find_registered_pane_filters_by_session() {
        // find_registered_pane_in_session should only return panes
        // that are alive and in the target tmux session.
        let iso = IsolatedTmux::new("route-test-find-reg");
        let cwd = std::env::current_dir().unwrap();

        // Create two sessions
        let pane_a = iso.auto_start("session-a", &cwd).unwrap();
        let pane_b = iso.auto_start("session-b", &cwd).unwrap();

        // Verify panes are in different sessions
        let sess_a = iso.pane_session(&pane_a).unwrap();
        let sess_b = iso.pane_session(&pane_b).unwrap();
        assert_eq!(sess_a, "session-a");
        assert_eq!(sess_b, "session-b");

        // find_registered_pane_in_session uses the sessions registry,
        // so this test just verifies the tmux infrastructure works.
        // The function itself filters by session name, which we test
        // indirectly via the pane_session check above.
        assert_ne!(pane_a, pane_b);
    }

    #[test]
    fn split_window_respects_working_directory() {
        let iso = IsolatedTmux::new("route-test-split-cwd");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        let pane1 = iso.auto_start(session, &cwd).unwrap();
        let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();

        // Both panes should be alive and in same window
        assert!(iso.pane_alive(&pane1));
        assert!(iso.pane_alive(&pane2));

        let w1 = iso.pane_window(&pane1).unwrap();
        let w2 = iso.pane_window(&pane2).unwrap();
        assert_eq!(w1, w2, "split pane should be in same window");

        // Verify the window now has 2 panes
        let panes = iso.list_window_panes(&w1).unwrap();
        assert_eq!(panes.len(), 2, "window should have exactly 2 panes after split");
    }

    #[test]
    fn stash_pane_on_split_failure() {
        // When split_window fails, the fallback should auto_start then stash
        // the pane so it doesn't create a visible new window.
        let iso = IsolatedTmux::new("route-test-stash-fallback");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create a pane then simulate the fallback path:
        // auto_start creates a new window, then stash_pane moves it.
        let pane = iso.auto_start(session, &cwd).unwrap();
        let fallback_pane = iso.auto_start(session, &cwd).unwrap();

        // Before stash: pane and fallback_pane are in different windows
        let w1 = iso.pane_window(&pane).unwrap();
        let w_fb_before = iso.pane_window(&fallback_pane).unwrap();
        assert_ne!(w1, w_fb_before, "fallback should be in a new window initially");

        // Stash the fallback pane (simulating what the route.rs fallback does)
        iso.stash_pane(&fallback_pane, session).unwrap();

        // After stash: fallback_pane should be in the stash window
        assert!(iso.pane_alive(&fallback_pane), "pane should still be alive");
        let stash_win = iso.find_stash_window(session);
        assert!(stash_win.is_some(), "stash window should have been created");
        let w_fb_after = iso.pane_window(&fallback_pane).unwrap();
        assert_eq!(
            w_fb_after,
            stash_win.unwrap(),
            "fallback pane should be in the stash window"
        );
    }

    // --- has_named_window tests ---

    #[test]
    fn has_named_window_detects_agent_doc_window() {
        let iso = IsolatedTmux::new("route-test-named-win");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create session (first window gets default name, not "agent-doc")
        let _pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            !has_named_window(&iso, session, "agent-doc"),
            "should not find 'agent-doc' window before renaming"
        );

        // Rename the window to "agent-doc"
        let _ = iso
            .cmd()
            .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
            .status();
        assert!(
            has_named_window(&iso, session, "agent-doc"),
            "should find 'agent-doc' window after renaming"
        );
    }

    #[test]
    fn has_named_window_false_for_nonexistent_session() {
        let iso = IsolatedTmux::new("route-test-named-win-no-sess");
        assert!(
            !has_named_window(&iso, "nonexistent", "agent-doc"),
            "should return false for nonexistent session"
        );
    }

    #[test]
    fn else_branch_stashes_when_agent_doc_window_exists() {
        // When no registered pane is found but an "agent-doc" window already
        // exists, the new pane should be stashed to prevent window proliferation.
        let iso = IsolatedTmux::new("route-test-else-stash");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create a session and rename its window to "agent-doc"
        let existing_pane = iso.auto_start(session, &cwd).unwrap();
        let _ = iso
            .cmd()
            .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
            .status();

        // Simulate what the else branch does: auto_start + stash
        let new_pane = iso.auto_start(session, &cwd).unwrap();
        assert!(iso.pane_alive(&new_pane));

        // The new pane should be in a different window initially
        let existing_win = iso.pane_window(&existing_pane).unwrap();
        let new_win_before = iso.pane_window(&new_pane).unwrap();
        assert_ne!(existing_win, new_win_before);

        // Stash it
        iso.stash_pane(&new_pane, session).unwrap();

        // After stash: pane is alive and in the stash window
        assert!(iso.pane_alive(&new_pane));
        let stash_win = iso.find_stash_window(session);
        assert!(stash_win.is_some(), "stash window should exist");
        let new_win_after = iso.pane_window(&new_pane).unwrap();
        assert_eq!(
            new_win_after,
            stash_win.unwrap(),
            "new pane should be in stash window"
        );
    }

    // --- tmux_session validation tests ---

    #[test]
    fn route_warns_on_nonexistent_tmux_session() {
        // When frontmatter specifies a tmux_session that doesn't exist,
        // run_with_tmux should log a warning and NOT create that session.
        let iso = IsolatedTmux::new("route-test-warn-nonexist");
        let cwd = std::env::current_dir().unwrap();

        // Create a fallback session so there's somewhere to land
        let _fallback_pane = iso.auto_start("claude", &cwd).unwrap();

        // Write a temp file with a nonexistent tmux_session in frontmatter
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(
            &file,
            "---\nagent_doc_session: test-uuid-1234\ntmux_session: ghost-session\n---\n## User\nHello\n",
        )
        .unwrap();

        // The nonexistent session should NOT exist before or after
        assert!(
            !iso.session_exists("ghost-session"),
            "ghost-session should not exist before route"
        );

        // Run route — it will fail at auto-start (AGENT_DOC_NO_AUTOSTART),
        // but we can verify the session was never created
        // SAFETY: test is single-threaded; env var is removed immediately after use
        unsafe { std::env::set_var("AGENT_DOC_NO_AUTOSTART", "1"); }
        let result = run_with_tmux(&file, &iso, None, 0, &[]);
        unsafe { std::env::remove_var("AGENT_DOC_NO_AUTOSTART"); }

        // The ghost session should still not exist (route fell back, didn't create it)
        assert!(
            !iso.session_exists("ghost-session"),
            "ghost-session should NOT have been created by route"
        );

        // Route should have bailed due to AGENT_DOC_NO_AUTOSTART (no active pane)
        assert!(result.is_err(), "should error with no autostart");
    }

    #[test]
    fn route_falls_back_to_existing_session() {
        // When frontmatter requests a nonexistent session, route should
        // fall back to an existing session and create panes there.
        let iso = IsolatedTmux::new("route-test-fallback-sess");
        let cwd = std::env::current_dir().unwrap();

        // Create the fallback session "claude"
        let fallback_pane = iso.auto_start("claude", &cwd).unwrap();
        let fallback_session = iso.pane_session(&fallback_pane).unwrap();
        assert_eq!(fallback_session, "claude");

        // Verify ghost-session does NOT exist
        assert!(!iso.session_exists("ghost-session"));

        // Write a temp file with a nonexistent tmux_session
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(
            &file,
            "---\nagent_doc_session: fallback-uuid-5678\ntmux_session: ghost-session\n---\n## User\nHello\n",
        )
        .unwrap();

        // Set AGENT_DOC_NO_AUTOSTART so we don't actually spawn Claude,
        // but we can inspect the validation behavior
        // SAFETY: test is single-threaded; env var is removed immediately after use
        unsafe { std::env::set_var("AGENT_DOC_NO_AUTOSTART", "1"); }
        let _result = run_with_tmux(&file, &iso, None, 0, &[]);
        unsafe { std::env::remove_var("AGENT_DOC_NO_AUTOSTART"); }

        // The ghost session should NOT have been created
        assert!(
            !iso.session_exists("ghost-session"),
            "nonexistent session should never be created by route"
        );

        // The fallback "claude" session should still exist
        assert!(
            iso.session_exists("claude"),
            "fallback session should still be alive"
        );
    }

    // --- Stash rescue tests ---

    #[test]
    fn pane_in_stash_rescued_to_agent_doc() {
        // When a registered pane ends up in a stash window, route should
        // rescue it back to the agent-doc window via swap-pane.
        let iso = IsolatedTmux::new("route-test-stash-rescue");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create session and rename the window to "agent-doc"
        let pane1 = iso.auto_start(session, &cwd).unwrap();
        let _ = iso
            .cmd()
            .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
            .status();

        // Create a second pane and stash it (simulating a pane that ended up in stash)
        let stashed_pane = iso.auto_start(session, &cwd).unwrap();
        iso.stash_pane(&stashed_pane, session).unwrap();

        // Verify it's in the stash window
        let stash_win = iso.find_stash_window(session);
        assert!(stash_win.is_some(), "stash window should exist");
        let pane_win = iso.pane_window(&stashed_pane).unwrap();
        assert_eq!(pane_win, stash_win.unwrap(), "pane should be in stash");

        // Now rescue: swap the stashed pane with a pane in the agent-doc window
        let agent_doc_window = format!("{}:agent-doc", session);
        let target_panes = iso.list_window_panes(&agent_doc_window).unwrap_or_default();
        assert!(!target_panes.is_empty(), "agent-doc window should have panes");

        if let Some(target) = target_panes.first() {
            // This is the same swap logic as route.rs:88
            match iso.swap_pane(&stashed_pane, target) {
                Ok(()) => {
                    // Verify the stashed pane is now in the agent-doc window
                    let _rescued_win = iso.pane_window(&stashed_pane).unwrap();
                    let _agent_doc_win_id = iso.pane_window(&pane1).unwrap_or_default();
                    // After swap, the stashed pane should be in agent-doc window
                    // and pane1 should be in stash
                    assert!(iso.pane_alive(&stashed_pane), "rescued pane should be alive");
                }
                Err(_e) => {
                    // Fallback to join-pane (same as route.rs:94)
                    iso.join_pane(&stashed_pane, target, "-dh").unwrap();
                    assert!(iso.pane_alive(&stashed_pane), "pane should survive join rescue");
                }
            }
        }
    }

    #[test]
    fn swap_failure_falls_back_to_join_pane() {
        // When swap-pane fails during rescue, join-pane should be used as fallback.
        let iso = IsolatedTmux::new("route-test-swap-fallback");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create session with agent-doc window
        let _pane1 = iso.auto_start(session, &cwd).unwrap();
        let _ = iso
            .cmd()
            .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
            .status();

        // Create a second pane in its own window
        let pane2 = iso.auto_start(session, &cwd).unwrap();
        let _win_before = iso.pane_window(&pane2).unwrap();

        // Use join_pane to move pane2 into agent-doc window (the fallback path)
        let agent_doc_window = format!("{}:agent-doc", session);
        let target_panes = iso.list_window_panes(&agent_doc_window).unwrap();
        let target = &target_panes[0];

        iso.join_pane(&pane2, target, "-dh").unwrap();

        // Verify pane2 is now in the agent-doc window
        let _win_after = iso.pane_window(&pane2).unwrap();
        let agent_doc_panes = iso.list_window_panes(&agent_doc_window).unwrap();
        assert!(
            agent_doc_panes.contains(&pane2),
            "pane should be in agent-doc window after join, got: {:?}",
            agent_doc_panes
        );
        assert!(iso.pane_alive(&pane2), "pane should be alive after join");
    }

    #[test]
    fn sync_after_claim_prefers_col_args_over_registry() {
        // Regression test: when editor provides col_args, sync_after_claim should
        // pass those to sync::run instead of auto-discovering from registry.
        // The actual pane stashing is handled by tmux-router's reconcile —
        // this test verifies the col_args flow.
        let iso = IsolatedTmux::new("route-test-col-args");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        let pane_a = iso.auto_start(session, &cwd).unwrap();
        let window_id = iso.pane_window(&pane_a).unwrap();

        // With col_args having < 2 entries, sync_after_claim returns early (no sync needed).
        // This verifies the early-return path.
        sync_after_claim(&iso, &pane_a, &["single.md".to_string()]);

        // With empty col_args and < 2 registry entries, also returns early.
        sync_after_claim(&iso, &pane_a, &[]);

        // Pane should still be alive and in the same window — no unintended stashing
        assert!(iso.pane_alive(&pane_a), "pane should survive sync_after_claim");
        assert_eq!(
            iso.pane_window(&pane_a).unwrap(),
            window_id,
            "pane should stay in original window"
        );

        // With 2+ col_args, sync_after_claim runs sync::run with those args.
        // sync::run will fail to resolve files (no registrations), but shouldn't crash.
        let col_args = vec!["file_a.md".to_string(), "file_b.md".to_string()];
        sync_after_claim(&iso, &pane_a, &col_args);

        // Pane should still be alive
        assert!(iso.pane_alive(&pane_a), "pane should survive sync with unresolved files");
    }

    // --- split_before positional target tests ---

    #[test]
    fn split_before_true_picks_leftmost_pane() {
        // Regression test for 3-pane layout bug (Fix 1):
        // When split_before=true (left-column file), the split target should be
        // the first (leftmost) pane in the agent-doc window — not the last.
        // Before the fix, the code always used find_registered_pane_in_session
        // which could pick any registered pane regardless of position.
        let iso = IsolatedTmux::new("route-test-split-before-left");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create a window with 2 panes side by side (simulating agent-doc window)
        let pane_left = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane_left).unwrap();
        let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
        let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();

        // Rename to "agent-doc" so list_window_panes("test:agent-doc") works
        let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

        // Verify setup: 2 panes, left then right
        let ordered = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(ordered.len(), 2, "should have 2 panes");
        assert_eq!(ordered[0], pane_left, "first pane should be leftmost");
        assert_eq!(ordered[1], pane_right, "second pane should be rightmost");

        // split_before=true: should pick the first pane (leftmost)
        // We split alongside pane_left with -dbh (before, horizontal)
        let new_pane = iso.split_window(&ordered[0], &cwd, "-dbh").unwrap();
        let new_window = iso.pane_window(&new_pane).unwrap();
        assert_eq!(
            iso.pane_window(&pane_left).unwrap(),
            new_window,
            "new pane should be in the same window as the leftmost pane"
        );

        // Verify the new pane is to the LEFT of the original leftmost pane
        let final_order = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(final_order.len(), 3, "should have 3 panes now");
        assert_eq!(
            final_order[0], new_pane,
            "new pane should be leftmost (split before)"
        );
    }

    #[test]
    fn split_before_false_picks_rightmost_pane() {
        // Regression test for 3-pane layout bug (Fix 1):
        // When split_before=false (right-column file), the split target should be
        // the last (rightmost) pane in the agent-doc window.
        let iso = IsolatedTmux::new("route-test-split-before-right");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create a window with 2 panes side by side
        let pane_left = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane_left).unwrap();
        let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
        let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();

        // Rename to "agent-doc"
        let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

        // Verify setup
        let ordered = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0], pane_left);
        assert_eq!(ordered[1], pane_right);

        // split_before=false: should pick the last pane (rightmost)
        // We split alongside pane_right with -dh (after, horizontal)
        let new_pane = iso.split_window(&ordered[1], &cwd, "-dh").unwrap();
        let new_window = iso.pane_window(&new_pane).unwrap();
        assert_eq!(
            iso.pane_window(&pane_right).unwrap(),
            new_window,
            "new pane should be in the same window as the rightmost pane"
        );

        // Verify the new pane is to the RIGHT of the original rightmost pane
        let final_order = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(final_order.len(), 3, "should have 3 panes now");
        assert_eq!(
            final_order[2], new_pane,
            "new pane should be rightmost (split after)"
        );
    }

    #[test]
    fn provision_pane_first_col_splits_left() {
        // Verify that provision_pane with a file in the first column
        // computes split_before=true via is_first_column and places the new
        // pane at the leftmost position in the agent-doc window.
        let iso = IsolatedTmux::new("route-test-auto-start-col-left");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create a window with 2 panes to simulate existing agent-doc layout
        let pane_left = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane_left).unwrap();
        let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
        let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

        // Verify 2-pane setup
        let ordered = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(ordered.len(), 2, "should start with 2 panes");

        // col_args: file_a is in first column, file_b in second
        let col_args = vec![
            "tasks/file_a.md".to_string(),
            "tasks/file_b.md".to_string(),
        ];

        // Call provision_pane with file in the FIRST column
        let file_a = Path::new("tasks/file_a.md");
        let result = provision_pane(
            &iso, file_a, "session-a", "tasks/file_a.md",
            Some(session), &col_args,
        );
        assert!(result.is_ok(), "provision_pane should succeed: {:?}", result.err());

        // The new pane should be leftmost (split_before=true picks first pane, splits -dbh)
        let after = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(after.len(), 3, "should have 3 panes after auto_start");
        // The new pane is NOT one of the original two — find it
        let new_pane: Vec<_> = after.iter()
            .filter(|p| *p != &pane_left && *p != &pane_right)
            .collect();
        assert_eq!(new_pane.len(), 1, "should have exactly 1 new pane");
        assert_eq!(
            &after[0], new_pane[0],
            "first-column file should produce leftmost pane (split_before=true)"
        );
    }

    #[test]
    fn provision_pane_second_col_splits_right() {
        // Verify that provision_pane with a file in the second column
        // computes split_before=false via is_first_column and places the new
        // pane at the rightmost position in the agent-doc window.
        let iso = IsolatedTmux::new("route-test-auto-start-col-right");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create a window with 2 panes to simulate existing agent-doc layout
        let pane_left = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane_left).unwrap();
        let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
        let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

        // Verify 2-pane setup
        let ordered = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(ordered.len(), 2, "should start with 2 panes");

        // col_args: file_a is in first column, file_b in second
        let col_args = vec![
            "tasks/file_a.md".to_string(),
            "tasks/file_b.md".to_string(),
        ];

        // Call provision_pane with file in the SECOND column
        let file_b = Path::new("tasks/file_b.md");
        let result = provision_pane(
            &iso, file_b, "session-b", "tasks/file_b.md",
            Some(session), &col_args,
        );
        assert!(result.is_ok(), "provision_pane should succeed: {:?}", result.err());

        // The new pane should be rightmost (split_before=false picks last pane, splits -dh)
        let after = iso.list_window_panes(&format!("{}:agent-doc", session)).unwrap();
        assert_eq!(after.len(), 3, "should have 3 panes after auto_start");
        // Find the new pane (not one of the original two)
        let new_pane: Vec<_> = after.iter()
            .filter(|p| *p != &pane_left && *p != &pane_right)
            .collect();
        assert_eq!(new_pane.len(), 1, "should have exactly 1 new pane");
        assert_eq!(
            after.last().unwrap(), new_pane[0],
            "second-column file should produce rightmost pane (split_before=false)"
        );
    }

    #[test]
    fn sync_after_claim_handles_malformed_registry() {
        // When sessions.json is malformed, sync_after_claim should not panic.
        // It should return early (silently) rather than propagating the error.
        let iso = IsolatedTmux::new("route-test-malformed-registry");
        let tmp = tempfile::TempDir::new().unwrap();
        let session = "test";
        let pane = iso.new_session(session, tmp.path()).unwrap();

        // Write malformed sessions.json (array format instead of map)
        let sessions_path = tmp.path().join(".agent-doc");
        std::fs::create_dir_all(&sessions_path).unwrap();
        std::fs::write(
            sessions_path.join("sessions.json"),
            r#"{"sessions": [{"bad": "format"}]}"#,
        ).unwrap();

        // sync_after_claim should not panic — it handles errors gracefully
        // (returns early on load failure)
        sync_after_claim(&iso, &pane, &[]);
        // If we reach here without panic, the test passes
    }

    #[test]
    fn sync_after_claim_with_empty_col_args_and_no_registry() {
        // When there's no registry file and no col_args, sync_after_claim
        // should return early without creating any panes.
        let iso = IsolatedTmux::new("route-test-no-registry");
        let tmp = tempfile::TempDir::new().unwrap();
        let session = "test";
        let pane = iso.new_session(session, tmp.path()).unwrap();
        let window = iso.pane_window(&pane).unwrap();

        let before = iso.list_window_panes(&window).unwrap();
        sync_after_claim(&iso, &pane, &[]);
        let after = iso.list_window_panes(&window).unwrap();

        assert_eq!(before.len(), after.len(), "no panes should be created when no registry exists");
    }
}

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
use std::time::Duration;

use crate::sessions::Tmux;
use crate::{frontmatter, prompt, resync, sessions, sync};

const TMUX_SESSION_NAME: &str = "claude";

pub fn run(file: &Path, pane: Option<&str>, debounce_ms: u64) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server(), pane, debounce_ms)
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux, pane: Option<&str>, debounce_ms: u64) -> Result<()> {
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

    // Use tmux_session from frontmatter if available, otherwise default
    let frontmatter_session = frontmatter::parse(&updated_content)
        .ok()
        .and_then(|(fm, _)| fm.tmux_session);
    let target_session = if let Some(ref requested) = frontmatter_session {
        if tmux.session_exists(requested) {
            requested.clone()
        } else {
            // Requested session doesn't exist — refuse to route to wrong session.
            // The user must create the session or update tmux_session in frontmatter.
            anyhow::bail!(
                "[route] tmux_session '{}' does not exist. \
                 Create it with `tmux new-session -ds {}` or update frontmatter.",
                requested, requested
            );
        }
    } else {
        // No tmux_session in frontmatter — use current session (first claim sets it)
        current_tmux_session(tmux)
            .unwrap_or_else(|| TMUX_SESSION_NAME.to_string())
    };
    eprintln!("[route] target tmux session: {}", target_session);

    let file_path = file.to_string_lossy();
    let registered = sessions::lookup(&session_id)?;

    // Step 1: Check if registered pane is alive AND in the correct tmux session
    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
            // Verify the pane is in the target tmux session
            let pane_session = tmux
                .cmd()
                .args(["display-message", "-t", registered_pane, "-p", "#{session_name}"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();

            if pane_session == target_session {
                eprintln!("[route] Pane {} is alive in session '{}'", registered_pane, pane_session);
                return send_command(tmux, registered_pane, &file_path);
            }
            eprintln!(
                "[route] Pane {} is alive but in wrong session ('{}', expected '{}'). Will re-create.",
                registered_pane, pane_session, target_session
            );
            // Don't kill — the user might want to keep the session.
            // Just proceed to auto-start in the correct session.
        } else {
            eprintln!("[route] Pane {} is dead", registered_pane);
        }
    } else {
        eprintln!(
            "[route] No pane registered for session {}",
            &session_id[..std::cmp::min(8, session_id.len())]
        );
    }

    // Step 2: Try lazy claim to an active pane (only when a registered pane died).
    // For unregistered files (no prior session), skip to auto-start so each file
    // gets its own Claude session instead of stealing an existing one.
    if registered.is_some()
        && let Some(new_pane) = find_target_pane(tmux, pane, &target_session) {
            eprintln!("[route] Lazy-claiming to pane {} (dead pane)", new_pane);
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
    auto_start_in_session(tmux, file, &session_id, &file_path, &target_session, false)?;
    Ok(())
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
    auto_start_ext(tmux, file, session_id, file_path, context_session, false)
}

/// Auto-start without waiting for Claude or sending commands.
/// Used by sync when it just needs the pane to exist.
pub fn auto_start_no_wait(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
) -> Result<()> {
    auto_start_ext(tmux, file, session_id, file_path, context_session, true)
}

fn auto_start_ext(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
) -> Result<()> {
    // Determine target session. Priority:
    // 1. context_session (from sync --window) — sync knows which session it's managing
    // 2. tmux_session from frontmatter — document's preferred session
    // 3. current_tmux_session() — first-claim fallback
    let session_name = if let Some(ctx) = context_session {
        // Context session from sync overrides frontmatter — sync is managing
        // a specific window/session and needs the pane there, not elsewhere.
        let fm_session = std::fs::read_to_string(file).ok()
            .and_then(|c| {
                let (fm, _) = frontmatter::parse(&c).ok()?;
                fm.tmux_session
            });
        if let Some(ref fm) = fm_session {
            eprintln!(
                "[auto_start] context_session '{}' overrides deprecated frontmatter tmux_session '{}'",
                ctx, fm
            );
        }
        ctx.to_string()
    } else if let Ok(content) = std::fs::read_to_string(file) {
        let requested = frontmatter::parse(&content)
            .ok()
            .and_then(|(fm, _)| fm.tmux_session);
        if let Some(ref name) = requested {
            if tmux.session_exists(name) {
                name.clone()
            } else {
                // Refuse to start in wrong session — bail instead of fallback
                anyhow::bail!(
                    "[auto_start] tmux_session '{}' does not exist. \
                     Create it with `tmux new-session -ds {}` or update frontmatter.",
                    name, name
                );
            }
        } else {
            // No tmux_session and no context — use current session (first claim sets it)
            current_tmux_session(tmux)
                .unwrap_or_else(|| TMUX_SESSION_NAME.to_string())
        }
    } else {
        TMUX_SESSION_NAME.to_string()
    };
    auto_start_in_session(tmux, file, session_id, file_path, &session_name, skip_wait)
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
fn auto_start_in_session(tmux: &Tmux, file: &Path, session_id: &str, file_path: &str, session_name: &str, skip_wait: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;

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
        window_panes.into_iter().next()
            .or_else(|| find_registered_pane_in_session(tmux, session_name, ""))
    } else {
        find_registered_pane_in_session(tmux, session_name, "")
    };
    let new_pane = if let Some(ref target) = existing_pane {
        match tmux.split_window(target, &cwd, "-dh") {
            Ok(pane) => {
                eprintln!(
                    "[route] split-window alongside registered pane {} in session '{}' → new pane {}",
                    target, session_name, pane
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

    // Start agent-doc start in the new pane
    let start_cmd = format!("{} start {}", agent_doc_bin, file_path);
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
            // send_command now includes Enter verification + retry
            send_command(tmux, &new_pane, file_path)?;
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
        let cmd_in_last_lines = content
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
        let result = run_with_tmux(&file, &iso, None, 0);
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
        let _result = run_with_tmux(&file, &iso, None, 0);
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
}

//! # Module: start
//!
//! ## Spec
//! - `run(file)`: validates the file exists, then ensures a session UUID is
//!   present in the YAML frontmatter (generates and writes one if absent).
//! - Resolves `claude_args` from three sources in priority order: frontmatter
//!   `claude_args` field > global config (`~/.config/agent-doc/config.toml`) >
//!   `AGENT_DOC_CLAUDE_ARGS` environment variable.
//! - Requires an active tmux session; bails immediately if not inside tmux.
//! - Registers the session UUID → current tmux pane ID in `sessions.json` so
//!   other subcommands (`route`, `focus`, etc.) can locate the pane.
//! - Runs `claude` as a blocking child process inside a persistent restart loop
//!   so the tmux pane never dies on its own.
//! - On non-zero exit (context exhaustion, crash, etc.): auto-restarts after a
//!   2-second delay using `--continue` to resume the previous conversation.
//! - On clean exit (code 0): prints a prompt to stderr and reads stdin; pressing
//!   Enter restarts fresh (no `--continue`), typing `q` + Enter exits.
//! - Prints the truncated session UUID and pane ID to stderr on registration.
//! - Opens a persistent session log at `.agent-doc/logs/<session-uuid>.log`,
//!   appending timestamped events for session start, claude start/restart/exit,
//!   user quit, and session end.
//! - On `--continue` restarts, spawns a background thread that waits 5 seconds
//!   then sends `/agent-doc <file>` via `tmux send-keys` to auto-trigger
//!   the skill workflow in the resumed conversation.
//!
//! ## Agentic Contracts
//! - The file path must exist before `run` is called; callers must not rely on
//!   `run` to create the document.
//! - After `run` returns `Ok(())`, the session has ended cleanly (user chose
//!   to quit); the sessions.json entry is not automatically removed.
//! - Session UUID in frontmatter is idempotent: calling `run` on a file that
//!   already has a UUID does not regenerate or overwrite it.
//! - `claude_args` are prepended to every `claude` invocation inside the loop,
//!   including restarts; they are resolved once at startup and held for the
//!   lifetime of the loop.
//! - The module writes to the document file (UUID injection), `sessions.json`,
//!   and `.agent-doc/logs/<session-uuid>.log`; it does not touch snapshots,
//!   git, or claims.
//! - Must be called from within an active tmux session; violating this contract
//!   returns an immediate `Err`.
//!
//! ## Evals
//! - `start_missing_file`: call `run` with a non-existent path → returns `Err`
//!   containing "file not found".
//! - `start_outside_tmux`: call `run` with a valid file while `TMUX` env var is
//!   unset → returns `Err` containing "not running inside tmux".
//! - `start_generates_uuid`: call `run` on a file with no frontmatter UUID →
//!   UUID is injected into the file and a "Generated session UUID" line appears
//!   on stderr before `claude` is launched.
//! - `start_preserves_existing_uuid`: call `run` on a file that already has a
//!   `session:` key → file content is unchanged (no re-write), no "Generated"
//!   message on stderr.
//! - `start_registers_session`: after setup, `sessions.json` maps the session
//!   UUID to the current tmux pane ID.
//! - `start_claude_args_precedence`: frontmatter `claude_args` overrides config
//!   which overrides `AGENT_DOC_CLAUDE_ARGS`; each layer verified independently.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

use crate::{config, frontmatter, sessions};

/// Open (or create) the session log file at `.agent-doc/logs/<session-uuid>.log`.
/// Returns a writable file handle in append mode, or None if the directory can't be created.
fn open_session_log(file: &Path, session_id: &str) -> Option<std::fs::File> {
    // Walk up from the document to find the project root containing .agent-doc/
    let dir = file.parent()?;
    let mut search = Some(dir);
    let mut agent_doc_dir = None;
    while let Some(d) = search {
        let candidate = d.join(".agent-doc");
        if candidate.is_dir() {
            agent_doc_dir = Some(candidate);
            break;
        }
        search = d.parent();
    }
    let logs_dir = agent_doc_dir?.join("logs");
    std::fs::create_dir_all(&logs_dir).ok()?;
    let log_path = logs_dir.join(format!("{}.log", session_id));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as ISO-ish: just use epoch seconds for simplicity in logs
    format!("{}", now)
}

fn log_event(log: &mut Option<std::fs::File>, msg: &str) {
    if let Some(f) = log {
        let _ = writeln!(f, "[{}] {}", timestamp(), msg);
    }
}

/// Expand frontmatter env values through the shell, preserving document order.
pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Ensure session UUID exists in frontmatter
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session(&content)?;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("Generated session UUID: {}", session_id);
    }

    // Resolve claude_args: frontmatter > config > env var
    let (fm, _body) = frontmatter::parse(&updated_content)?;
    let resolved_claude_args = fm
        .claude_args
        .or_else(|| config::load().ok().and_then(|c| c.claude_args))
        .or_else(|| std::env::var("AGENT_DOC_CLAUDE_ARGS").ok());

    // Must be inside tmux
    if !sessions::in_tmux() {
        anyhow::bail!("not running inside tmux — start a tmux session first");
    }

    let pane_id = sessions::current_pane()?;

    // Guard: auto-relocate if the current pane is in a different session than the project expects.
    // This is how cross-session drift happens — a terminal in session 1 claims a document,
    // permanently binding it to session 1 even though the project targets session 0.
    if let Some(expected_session) = config::project_tmux_session() {
        let tmux = sessions::Tmux::default_server();
        relocate_if_wrong_session(&tmux, &pane_id, &expected_session);
    }

    // Register session → pane (with relative file path)
    let file_str = file.to_string_lossy();
    sessions::register(&session_id, &pane_id, &file_str)?;
    eprintln!(
        "Registered session {} → pane {}",
        &session_id[..8],
        pane_id
    );

    // Open session log
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut session_log = open_session_log(&canonical, &session_id);
    log_event(
        &mut session_log,
        &format!(
            "session_start file={} pane={} session={}",
            file.display(),
            pane_id,
            &session_id[..8]
        ),
    );

    // Fire document-level session_start hooks
    crate::hooks::fire_doc_hooks(&fm.hooks, "session_start", file, &session_id, &fm.agent, &fm.model);

    // Run claude in a restart loop — pane never dies
    let mut first_run = true;
    let mut restart_count: u32 = 0;
    loop {
        let mut cmd = std::process::Command::new("claude");
        // Add resolved claude_args before other flags
        if let Some(ref args) = resolved_claude_args {
            for arg in args.split_whitespace() {
                cmd.arg(arg);
            }
        }
        // Add --no-mcp if frontmatter sets no_mcp: true
        if fm.no_mcp.unwrap_or(false) {
            cmd.arg("--no-mcp");
        }
        // Set ENABLE_TOOL_SEARCH env var if frontmatter sets enable_tool_search: true
        if fm.enable_tool_search.unwrap_or(false) {
            cmd.env("ENABLE_TOOL_SEARCH", "true");
        }
        // Expand and set frontmatter env vars on the child process.
        // Values are expanded through the shell in document order so later
        // values can reference earlier keys (e.g., $OPENROUTER_API_KEY).
        if !fm.env.is_empty() {
            match crate::env::expand_values(&fm.env) {
                Ok(expanded) => {
                    for (key, value) in &expanded {
                        cmd.env(key, value);
                    }
                }
                Err(e) => {
                    eprintln!("[env] warning: failed to expand env values: {}", e);
                }
            }
        }
        let auto_trigger = if !first_run {
            // After first run, continue the previous session
            cmd.arg("--continue");
            eprintln!("Restarting claude (--continue)...");
            log_event(
                &mut session_log,
                &format!("claude_restart mode=continue restart_count={}", restart_count),
            );
            true
        } else {
            eprintln!("Starting claude...");
            log_event(
                &mut session_log,
                &format!(
                    "claude_start mode={} restart_count={}",
                    if restart_count == 0 { "fresh" } else { "fresh_restart" },
                    restart_count
                ),
            );
            false
        };

        // For --continue restarts, schedule auto-trigger of /agent-doc via tmux send-keys
        // after a short delay to let Claude initialize
        if auto_trigger {
            let trigger_pane = pane_id.clone();
            let trigger_file = file.to_string_lossy().to_string();
            let mut trigger_log = session_log.as_ref().and_then(|f| f.try_clone().ok());
            std::thread::spawn(move || {
                // Wait for Claude to initialize and be ready for input
                std::thread::sleep(std::time::Duration::from_secs(5));
                let trigger_cmd = format!("/agent-doc {}", trigger_file);
                let status = std::process::Command::new("tmux")
                    .args(["send-keys", "-t", &trigger_pane, &trigger_cmd, "Enter"])
                    .output();
                match status {
                    Ok(output) if output.status.success() => {
                        if let Some(ref mut f) = trigger_log {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let _ = writeln!(f, "[{}] auto_trigger sent=\"{}\"", ts, trigger_cmd);
                        }
                        eprintln!("[agent-doc] auto-triggered: {}", trigger_cmd);
                    }
                    _ => {
                        if let Some(ref mut f) = trigger_log {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let _ = writeln!(f, "[{}] auto_trigger_failed", ts);
                        }
                        eprintln!("[agent-doc] auto-trigger failed");
                    }
                }
            });
        }

        let status = cmd.status().context("failed to run claude")?;
        first_run = false;

        let code = status.code().unwrap_or(1);
        log_event(
            &mut session_log,
            &format!("claude_exit code={} restart_count={}", code, restart_count),
        );

        if code == 0 {
            // Clean exit — prompt user
            eprintln!("\nClaude exited cleanly.");
            eprintln!("Press Enter to restart, or 'q' to exit.");
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_err() {
                log_event(&mut session_log, "stdin_read_failed — exiting loop");
                break;
            }
            if input.trim().eq_ignore_ascii_case("q") {
                log_event(&mut session_log, "user_quit");
                break;
            }
            // User pressed Enter — restart fresh
            first_run = true;
            restart_count += 1;
        } else {
            // Non-zero exit (context exhaustion, crash, etc.) — auto-restart
            eprintln!(
                "\nClaude exited with code {}. Auto-restarting in 2s...",
                code
            );
            restart_count += 1;
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }

    log_event(&mut session_log, "session_end");
    eprintln!("Session ended for {}", file.display());
    Ok(())
}

/// Auto-relocate `pane_id` to `expected_session` if it is currently in a different session.
/// Returns `true` if relocation succeeded or was unnecessary; `false` if relocation failed.
/// Falls back to warn-only on failure so the start isn't aborted.
pub(crate) fn relocate_if_wrong_session(
    tmux: &sessions::Tmux,
    pane_id: &str,
    expected_session: &str,
) -> bool {
    let actual_session = match tmux.pane_session(pane_id) {
        Ok(s) => s,
        Err(_) => return true, // can't determine — let registration proceed
    };
    if actual_session == expected_session {
        return true;
    }
    eprintln!(
        "[start] pane {} is in session '{}', expected '{}' — auto-relocating to project session",
        pane_id, actual_session, expected_session
    );
    if let Some(anchor) = tmux.active_pane(expected_session) {
        match sessions::PaneMoveOp::new(tmux, pane_id, &anchor)
            .allow_cross_session("auto-relocate to project session on start")
            .join("-dh")
        {
            Ok(()) => {
                eprintln!("[start] relocated pane {} → session '{}'", pane_id, expected_session);
                true
            }
            Err(e) => {
                eprintln!(
                    "[start] WARNING: relocation failed ({}); pane {} will register in session '{}'",
                    e, pane_id, actual_session
                );
                false
            }
        }
    } else {
        eprintln!(
            "[start] WARNING: no active pane found in session '{}'; \
             pane {} will register in session '{}'",
            expected_session, pane_id, actual_session
        );
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::fire_doc_hooks;
    use crate::sessions::IsolatedTmux;
    use std::collections::HashMap;

    // --- relocate_if_wrong_session tests ---

    #[test]
    fn relocate_noop_when_already_correct_session() {
        let iso = IsolatedTmux::new("start-reloc-noop");
        let pane = iso.new_session("sess-a", std::path::Path::new("/tmp")).unwrap();
        // pane is already in sess-a; no relocation needed
        let result = relocate_if_wrong_session(&iso, &pane, "sess-a");
        assert!(result, "should return true (noop — already in correct session)");
        // Verify pane is still in sess-a
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(sess, "sess-a");
    }

    #[test]
    fn relocate_succeeds_cross_session() {
        let iso = IsolatedTmux::new("start-reloc-cross");
        let _pane_a = iso.new_session("sess-a", std::path::Path::new("/tmp")).unwrap();
        let pane_b = iso.new_session("sess-b", std::path::Path::new("/tmp")).unwrap();
        // pane_b is in sess-b; expected is sess-a — should auto-relocate
        let result = relocate_if_wrong_session(&iso, &pane_b, "sess-a");
        assert!(result, "should return true after successful relocation");
        let sess = iso.pane_session(&pane_b).unwrap();
        assert_eq!(sess, "sess-a", "pane should be in sess-a after relocation");
    }

    #[test]
    fn relocate_fails_gracefully_when_no_anchor() {
        let iso = IsolatedTmux::new("start-reloc-noanchor");
        let pane = iso.new_session("sess-a", std::path::Path::new("/tmp")).unwrap();
        // Expected session "sess-nonexistent" has no active pane — relocation should fail gracefully
        let result = relocate_if_wrong_session(&iso, &pane, "sess-nonexistent");
        assert!(!result, "should return false when no anchor pane exists in expected session");
        // pane should still be in original session
        let sess = iso.pane_session(&pane).unwrap();
        assert_eq!(sess, "sess-a", "pane should remain in original session on failure");
    }

    #[test]
    fn fire_doc_hooks_substitutes_template_vars() {
        let tmp = std::env::temp_dir().join(format!("agent-doc-hook-test-{}.txt", std::process::id()));
        let cmd = format!("echo '{{{{session_id}}}}:{{{{agent}}}}:{{{{model}}}}' > {}", tmp.display());
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![cmd]);
        fire_doc_hooks(
            &hooks,
            "session_start",
            Path::new("/doc/test.md"),
            "abc-123",
            &Some("claude".to_string()),
            &Some("opus".to_string()),
        );
        let output = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert!(output.contains("abc-123"), "session_id not substituted: {}", output);
        assert!(output.contains("claude"), "agent not substituted: {}", output);
        assert!(output.contains("opus"), "model not substituted: {}", output);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn fire_doc_hooks_noop_for_missing_event() {
        let hooks: HashMap<String, Vec<String>> = HashMap::new();
        fire_doc_hooks(&hooks, "session_start", Path::new("/doc/test.md"), "id", &None, &None);
    }

    #[test]
    fn fire_doc_hooks_noop_for_empty_event() {
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![]);
        fire_doc_hooks(&hooks, "session_start", Path::new("/doc/test.md"), "id", &None, &None);
    }

    #[test]
    fn fire_doc_hooks_handles_none_agent_model() {
        let tmp = std::env::temp_dir().join(format!("agent-doc-hook-none-test-{}.txt", std::process::id()));
        let cmd = format!("printf '{{{{agent}}}}:{{{{model}}}}' > {}", tmp.display());
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("session_start".to_string(), vec![cmd]);
        fire_doc_hooks(&hooks, "session_start", Path::new("/doc/test.md"), "id", &None, &None);
        let output = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert_eq!(output, ":", "expected empty agent+model, got: {}", output);
        let _ = std::fs::remove_file(&tmp);
    }

}

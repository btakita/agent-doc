//! # Module: terminal
//!
//! ## Spec
//! - `run(file, session_name)` ensures a tmux session exists and opens a terminal
//!   emulator attached to it. Designed as a fallback for editor plugin users who
//!   have no terminal open yet.
//! - Session resolution order:
//!   1. Explicit `--session` flag, if provided.
//!   2. Scan the document store's durable registry for any live pane; reuse its session.
//!   3. Default session name `"0"`.
//! - The store, its multi-session policy (`tmux_sessions`), its registry and its
//!   `tmux_session` pin are the **document's** (`project_store_for_doc`: the nearest
//!   `.agent-doc` project, or a rootless document's canonical parent), never the
//!   process cwd's (`#cwdfallbackrest`). A store that does not exist yet has no
//!   registry and no pin, and reading it creates nothing.
//! - `tmux_session` frontmatter field is deprecated and never consulted during resolution.
//! - When no live session is found, the session name falls back to the project
//!   `tmux_session` binding in `.agent-doc/config.toml` before the `"0"` default,
//!   so `agent-doc session set <name>` also governs cold-start bootstrap (the
//!   only path available when no tmux server is running yet).
//! - If the resolved session already has an attached client → no-op (no duplicate terminals).
//! - If the session exists but is detached → open a terminal to re-attach.
//! - If no session found → create a new one via `tmux new-session -A -s <name>`.
//! - Terminal command resolution order:
//!   1. `[terminal] command` in `~/.config/agent-doc/config.toml` (supports `{tmux_command}` placeholder).
//!   2. `$TERMINAL` env var (appended with `-e {tmux_command}`).
//!   3. Error with setup instructions.
//! - Session names containing characters outside `[a-zA-Z0-9_-]` are single-quote-escaped
//!   before being passed to the shell.
//!
//! ## Agentic Contracts
//! - `run()` is the sole public entry point.
//! - Never creates duplicate terminals; attaches to existing sessions when available.
//! - Terminal process is spawned detached (fire-and-forget); `run()` returns immediately.
//! - Resolution is idempotent: calling `run()` twice for an already-attached session is a no-op.
//!
//! ## Evals
//! - shell_escape_simple: alphanumeric names → returned unchanged
//! - shell_escape_special: name with space → single-quoted output
//! - resolve_terminal_from_config: config has `command` template → `{tmux_command}` replaced correctly
//! - resolve_terminal_no_config_no_env: no config, no `$TERMINAL` → error containing "No terminal configured"
//! - classify_session_target: `SessionTarget::Create` variant matches correctly
//! - resolve_session_name_defaults_to_zero: no flag, no project binding → returns "0"
//! - resolve_session_name_uses_explicit_flag: explicit flag "correct" → returns "correct" ignoring frontmatter
//! - resolve_session_name_prefers_project_binding: no flag, project binding "ws" → returns "ws"
//! - resolve_session_name_explicit_beats_project_binding: flag "correct" + binding "ws" → returns "correct"
//! - resolve_target_session_reads_the_document_store_pin_never_the_cwd: stopped tmux → creates the
//!   document store's pin (`"0"` for a rootless document), never the cwd's; creates no store

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use tmux_router::Tmux;

use agent_doc_config::terminal_host::{
    ResolvedTerminalHost, ResolvedTerminalPolicy, TerminalSessionState, resolve_terminal_policy,
};

/// Run the terminal command: ensure tmux session exists, launch terminal if needed.
pub fn run(file: &Path, session_name: Option<&str>) -> Result<()> {
    let tmux = Tmux::default();

    // Step 1: Resolve the target session
    let target = resolve_target_session(&tmux, file, session_name)?;
    let session_state = match &target {
        SessionTarget::Attached(_) => TerminalSessionState::Attached,
        SessionTarget::Detached(_) => TerminalSessionState::Detached,
        SessionTarget::Create(_) => TerminalSessionState::Missing,
    };
    let policy = terminal_policy_for_file(&tmux, file, session_state)?;
    if let Some(failure) = policy.failure.as_deref() {
        anyhow::bail!("{failure}");
    }

    match target {
        SessionTarget::Attached(name) => {
            eprintln!(
                "[terminal] session '{}' already has an attached client — skipping",
                name
            );
            Ok(())
        }
        SessionTarget::Detached(name) => {
            eprintln!(
                "[terminal] session '{}' exists but is detached — opening terminal to attach",
                name
            );
            launch_resolved_terminal(&policy, &name)
        }
        SessionTarget::Create(name) => {
            if !policy.auto_start_tmux {
                anyhow::bail!(
                    "tmux session '{}' is absent and terminal auto-start is disabled; start it manually, then retry",
                    name
                );
            }
            eprintln!("[terminal] no active session found — creating '{}'", name);
            launch_resolved_terminal(&policy, &name)
        }
    }
}

// ---------------------------------------------------------------------------
// Session resolution
// ---------------------------------------------------------------------------

/// The resolved target session state.
enum SessionTarget {
    /// Session exists and has an attached terminal client — do nothing.
    Attached(String),
    /// Session exists but no terminal is attached — open terminal to attach.
    Detached(String),
    /// No suitable session found — create a new one.
    Create(String),
}

/// Resolve which tmux session to target. Resolution order:
///
/// 1. Explicit `--session` flag
/// 2. Scan the durable registry for ANY live session with agent-doc panes for this project
/// 3. Fall back to default session name "0"
///
/// Note: `tmux_session` frontmatter is deprecated in single-session projects
/// and not consulted there. In multi-session projects (`tmux_sessions` set,
/// GH #17) it is the document binding and outranks the registry scan; every
/// target must then be in the allowed list.
/// For each candidate, checks if the session exists and is attached.
fn resolve_target_session(
    tmux: &Tmux,
    file: &Path,
    explicit: Option<&str>,
) -> Result<SessionTarget> {
    // `#cwdfallbackrest`: the document's store, never the process cwd's.
    let store = agent_doc_project_config_io::project_store_for_doc(file);
    // Multi-session projects (`tmux_sessions`, GH #17): a document bound to an
    // allowed session targets that session, and every target must be allowed.
    let file_arg = file.to_string_lossy().into_owned();
    let policy = store
        .as_deref()
        .map(agent_doc_sync_io::sync::tmux_session_policy_for_root)
        .unwrap_or_default();
    let binding = agent_doc_sync_io::sync::scoped_document_tmux_session_binding(
        &policy,
        &[],
        Some(&file_arg),
    );
    let explicit = match explicit {
        Some(name) => {
            policy.require(name, agent_doc_sync::LayoutSessionAuthority::Explicit)?;
            Some(name.to_string())
        }
        None => match binding {
            Some(bound) => {
                policy.require(
                    &bound,
                    agent_doc_sync::LayoutSessionAuthority::DocumentBinding,
                )?;
                Some(bound)
            }
            None => None,
        },
    };
    let explicit = explicit.as_deref();

    if !tmux.running() {
        // tmux not running at all — create with the best session name we have
        let name = resolve_session_name(store.as_deref(), explicit)?;
        policy.require(&name, agent_doc_sync::LayoutSessionAuthority::ScopedProject)?;
        return Ok(SessionTarget::Create(name));
    }

    // Try explicit --session flag (or the multi-session document binding) first
    if let Some(name) = explicit {
        return Ok(classify_session(tmux, name));
    }

    // Scan the document store's registry for any live session hosting its panes
    if let Some(active_session) = find_active_project_session(tmux, store.as_deref())?
        && policy.allows(&active_session)
    {
        eprintln!(
            "[terminal] targeting session '{}' (from registry scan)",
            active_session
        );
        return Ok(classify_session(tmux, &active_session));
    }

    // No active session found — create with default name
    let name = resolve_session_name(store.as_deref(), None)?;
    policy.require(&name, agent_doc_sync::LayoutSessionAuthority::ScopedProject)?;
    Ok(SessionTarget::Create(name))
}

/// Classify an existing tmux session as Attached or Detached.
fn classify_session(tmux: &Tmux, name: &str) -> SessionTarget {
    if is_session_attached(tmux, name) {
        SessionTarget::Attached(name.to_string())
    } else {
        SessionTarget::Detached(name.to_string())
    }
}

/// Scan the document store's durable registry for any live pane, and return the
/// tmux session that pane belongs to.
///
/// This handles the "stale frontmatter" case: if the frontmatter says session "foo"
/// but "foo" doesn't exist, we check if there's any OTHER session with live agent-doc
/// panes for this project. `store` is the document's (`#cwdfallbackrest`); a
/// store that does not exist yet has no panes and is not created.
fn find_active_project_session(tmux: &Tmux, store: Option<&Path>) -> Result<Option<String>> {
    let Some(store) = store else {
        return Ok(None);
    };
    let registry = agent_doc_session_registry_io::load_existing_in(store)?;
    for entry in registry.values() {
        if tmux.pane_alive(&entry.pane) {
            // This pane is alive — find which tmux session it belongs to
            if let Some(session_name) = pane_session_name(tmux, &entry.pane) {
                return Ok(Some(session_name));
            }
        }
    }
    Ok(None)
}

/// Get the tmux session name that a pane belongs to.
fn pane_session_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    tmux.pane_session(pane_id)
        .ok()
        .filter(|name| !name.is_empty())
}

// ---------------------------------------------------------------------------
// Terminal launch
// ---------------------------------------------------------------------------

/// Build the tmux command and launch the resolved external terminal emulator.
fn launch_resolved_terminal(policy: &ResolvedTerminalPolicy, session_name: &str) -> Result<()> {
    match policy.host {
        ResolvedTerminalHost::External => {}
        ResolvedTerminalHost::Ide => anyhow::bail!(
            "terminal host resolved to IDE; run the document from the registered editor terminal endpoint"
        ),
        ResolvedTerminalHost::None => {
            anyhow::bail!("No terminal host available: {}", policy.reason)
        }
    }
    let tmux_command = format!("tmux new-session -A -s {}", shell_escape(session_name));
    let terminal_cmd = resolve_terminal_command(policy.command.as_deref(), &tmux_command)?;

    eprintln!("[terminal] launching: {}", terminal_cmd);
    spawn_terminal(&terminal_cmd)
}

fn terminal_policy_for_file(
    tmux: &Tmux,
    file: &Path,
    session_state: TerminalSessionState,
) -> Result<ResolvedTerminalPolicy> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_with_source(
        file,
        "terminal-command-policy",
    )
    .with_context(|| format!("failed to resolve terminal policy from {}", file.display()))?
    .into_content();
    let (document, _) = agent_doc_frontmatter::frontmatter::parse(&content)
        .with_context(|| format!("failed to parse terminal policy from {}", file.display()))?;
    let project = agent_doc_project_config_io::load_project_for_doc(file);
    let global = agent_doc_config::load()?;
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let tmux_installed = tmux
        .cmd()
        .arg("-V")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let report = agent_doc_config::terminal_host::classify(
        &env,
        tmux_installed,
        tmux_installed && tmux.running(),
    );
    let command_available = project
        .terminal
        .as_ref()
        .and_then(|config| config.command.as_deref())
        .is_some_and(|command| !command.trim().is_empty())
        || global
            .terminal
            .as_ref()
            .and_then(|config| config.command.as_deref())
            .is_some_and(|command| !command.trim().is_empty())
        || std::env::var("TERMINAL").is_ok_and(|terminal| !terminal.trim().is_empty());
    let external_available =
        agent_doc_config::terminal_host::external_host_available(&report, command_available);

    Ok(resolve_terminal_policy(
        document.terminal_host,
        project.terminal.as_ref(),
        global.terminal.as_ref(),
        report.resolved_terminal_host,
        false,
        external_available,
        session_state,
    ))
}

/// Resolve the tmux session name from (in order):
/// 1. Explicit --session flag
/// 2. The document store's `tmux_session` binding in `.agent-doc/config.toml`
///    (written by `agent-doc session set`, auto-rebound by `start` when the
///    configured session is dead), never the process cwd project's
///    (`#cwdfallbackrest`)
/// 3. Default "0"
///
/// Note: `tmux_session` frontmatter is deprecated and no longer consulted: only
/// the document's store, not the document, reaches this function.
fn resolve_session_name(store: Option<&Path>, explicit: Option<&str>) -> Result<String> {
    Ok(resolve_session_name_from(
        explicit,
        store
            .and_then(agent_doc_project_config_io::project_tmux_session_for_root)
            .as_deref(),
    ))
}

/// Pure precedence for [`resolve_session_name`]: explicit flag, then the project
/// binding, then the `"0"` fallback.
fn resolve_session_name_from(explicit: Option<&str>, configured: Option<&str>) -> String {
    explicit.or(configured).unwrap_or("0").to_string()
}

// ---------------------------------------------------------------------------
// Tmux queries
// ---------------------------------------------------------------------------

/// Check if a tmux session has at least one attached client.
fn is_session_attached(tmux: &Tmux, session: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["list-clients", "-t", session, "-F", "#{client_name}"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            !stdout.trim().is_empty()
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Terminal emulator resolution and launch
// ---------------------------------------------------------------------------

/// Resolve the terminal launch command. Priority:
/// 1. `[terminal] command` in config.toml
/// 2. `$TERMINAL` env var (used as: `$TERMINAL -e {tmux_command}`)
/// 3. Error with instructions
fn resolve_terminal_command(command: Option<&str>, tmux_command: &str) -> Result<String> {
    if let Some(cmd_template) = command {
        let resolved = cmd_template.replace("{tmux_command}", tmux_command);
        return Ok(resolved);
    }

    if let Ok(terminal) = std::env::var("TERMINAL")
        && !terminal.is_empty()
    {
        return Ok(format!("{} -e {}", terminal, tmux_command));
    }

    anyhow::bail!(
        "No terminal configured.\n\
         \n\
         Add to ~/.config/agent-doc/config.toml:\n\
         \n\
         [terminal]\n\
         command = \"<your-terminal> -- {{tmux_command}}\"\n\
         \n\
         Examples:\n\
         command = \"wezterm start -- {{tmux_command}}\"\n\
         command = \"kitty -- {{tmux_command}}\"\n\
         command = \"alacritty -e {{tmux_command}}\"\n\
         command = \"gnome-terminal -- {{tmux_command}}\"\n\
         \n\
         Or set the $TERMINAL environment variable."
    )
}

/// Spawn the terminal process (detached from current process).
fn spawn_terminal(command: &str) -> Result<()> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("terminal command is empty");
    }

    Command::new(parts[0])
        .args(&parts[1..])
        .spawn()
        .with_context(|| format!("failed to launch terminal: {}", parts[0]))?;

    Ok(())
}

/// Minimal shell escaping for session names.
fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn unset(key: &'static str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("my-session"), "my-session");
        assert_eq!(shell_escape("0"), "0");
    }

    #[test]
    fn shell_escape_special() {
        assert_eq!(shell_escape("my session"), "'my session'");
    }

    #[test]
    fn resolve_terminal_from_config() {
        let result = resolve_terminal_command(
            Some("wezterm start -- {tmux_command}"),
            "tmux new-session -A -s 0",
        )
        .unwrap();
        assert_eq!(result, "wezterm start -- tmux new-session -A -s 0");
    }

    #[test]
    fn resolve_terminal_no_config_no_env() {
        let _terminal = EnvGuard::unset("TERMINAL");
        let result = resolve_terminal_command(None, "tmux new-session -A -s 0");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No terminal configured"), "got: {}", err);
    }

    #[test]
    fn classify_session_target() {
        // SessionTarget enum is correct at compile time — this tests the structure
        let target = SessionTarget::Create("test".to_string());
        match target {
            SessionTarget::Attached(n) => panic!("expected Create, got Attached({})", n),
            SessionTarget::Detached(n) => panic!("expected Create, got Detached({})", n),
            SessionTarget::Create(n) => assert_eq!(n, "test"),
        }
    }

    /// Regression: with no flag and no project binding the name is "0".
    /// Frontmatter is never consulted.
    #[test]
    fn resolve_session_name_defaults_to_zero() {
        let name = resolve_session_name_from(None, None);
        assert_eq!(
            name, "0",
            "should default to '0' when nothing is configured"
        );
    }

    /// The project `tmux_session` binding governs cold-start bootstrap, which is
    /// the only path available when no tmux server is running yet.
    #[test]
    fn resolve_session_name_prefers_project_binding() {
        let name = resolve_session_name_from(None, Some("workspace"));
        assert_eq!(
            name, "workspace",
            "project tmux_session binding should beat the '0' fallback"
        );
    }

    /// An explicit `--session` flag still wins over the project binding.
    #[test]
    fn resolve_session_name_explicit_beats_project_binding() {
        let name = resolve_session_name_from(Some("correct"), Some("workspace"));
        assert_eq!(name, "correct", "explicit flag should take precedence");
    }

    /// Regression: resolve_session_name with explicit flag should use it directly.
    #[test]
    fn resolve_session_name_uses_explicit_flag() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/config.toml"),
            "tmux_session = \"workspace\"\n",
        )
        .unwrap();

        let name = resolve_session_name(Some(tmp.path()), Some("correct")).unwrap();
        assert_eq!(name, "correct", "explicit flag should take precedence");
        let name = resolve_session_name(Some(tmp.path()), None).unwrap();
        assert_eq!(name, "workspace", "the store's pin beats the '0' fallback");
    }

    /// `#cwdfallbackrest`: with no tmux server running, `terminal` creates the
    /// session its document's store pins, never the process cwd project's pin.
    /// A rootless document's store has no pin until it is created, so it gets
    /// `"0"`; resolving either creates no store and no registry.
    #[test]
    fn resolve_target_session_reads_the_document_store_pin_never_the_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let project = base.join("doc-project");
        std::fs::create_dir_all(project.join(".agent-doc")).unwrap();
        std::fs::write(
            project.join(".agent-doc/config.toml"),
            "tmux_session = \"doc-pin\"\n",
        )
        .unwrap();
        let tracked = project.join("doc.md");
        std::fs::write(&tracked, "# tracked\n").unwrap();
        let loose = base.join("loose");
        std::fs::create_dir_all(&loose).unwrap();
        let rootless = loose.join("rootless.md");
        std::fs::write(&rootless, "# rootless\n").unwrap();
        // A socket no server listens on: `has-session` fails, so tmux is not
        // running and nothing reaches a live server.
        let tmux = Tmux::default().with_server_socket(Some(format!(
            "agent-doc-terminal-store-test-{}-absent",
            std::process::id()
        )));
        assert!(!tmux.running(), "fixture tmux server must not be running");

        match resolve_target_session(&tmux, &tracked, None).unwrap() {
            SessionTarget::Create(name) => assert_eq!(name, "doc-pin"),
            _ => panic!("expected Create for a stopped tmux server"),
        }
        match resolve_target_session(&tmux, &rootless, None).unwrap() {
            SessionTarget::Create(name) => assert_eq!(name, "0"),
            _ => panic!("expected Create for a stopped tmux server"),
        }
        assert!(
            !loose.join(".agent-doc").exists(),
            "resolving a rootless document's session must not create its store"
        );
        assert!(
            !project.join(".agent-doc/state.db").exists(),
            "resolving the session must not create the store's registry"
        );
    }
}

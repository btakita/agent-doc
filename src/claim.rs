//! # Module: claim — Binding (explicit)
//!
//! `agent-doc claim` — create a **Binding** between a document and an existing tmux pane.
//!
//! **Ontology:** Claim creates a **Binding** (document→pane association) by registering
//! the session→pane mapping in `sessions.json`. Unlike **Provisioning** (which creates
//! new panes), claim binds to a pane that already exists. In normal editor workflow,
//! users don't need to call claim — **Reconciliation** (`sync`) + **Provisioning**
//! (`auto_start`) handle pane creation automatically. Claim is for manual pane assignment.
//!
//! Usage: `agent-doc claim <file.md> [--position left|right|top|bottom] [--pane %N] [--window @N]`
//!
//! Reads (or generates) the session UUID in the document's YAML frontmatter, resolves
//! the target pane, and registers the session→pane mapping in `sessions.json`. This
//! mapping is consumed by `agent-doc route` and the JetBrains/VS Code plugins to
//! direct commands to the correct tmux pane.
//!
//! ## Spec
//! - `run(file, position, pane, window, _force)` is the sole public entry point.
//! - Prunes stale registry entries via `resync::prune()` before any resolution.
//! - Calls `validate_file_claim(file)` to remove dead-pane entries for this specific
//!   file and log why the re-claim was needed (complements the bulk prune).
//! - Canonicalises the file path to handle CWD drift (e.g. when called from a
//!   submodule directory).
//! - Window resolution when `--window` is provided:
//!   1. Window is alive → use it directly.
//!   2. Window is dead → search `sessions.json` for an alive window in the same
//!      project CWD via `find_alive_project_window`. Falls through to no-window
//!      behaviour if none found.
//!   3. No `--window` → no window scoping.
//! - Ensures the session UUID exists in frontmatter via `frontmatter::ensure_session`;
//!   writes the UUID back to disk if it was freshly generated.
//! - Pane resolution priority: explicit `--pane` > `--position` (scoped to
//!   effective window if set) > `TMUX_PANE` / active pane.
//! - **Session validation:** After resolving `pane_id`, checks that the pane belongs to
//!   the project's configured tmux session (`project_tmux_session()`). Cross-session
//!   mismatches are auto-forced with a warning — explicit claim always succeeds.
//!   `--force` additionally overrides the binding invariant (commandeering another
//!   document's pane). This prevents the stash/rescue routing dance that fires
//!   whenever a document's pane is in the wrong session (`pane_session !=
//!   target_session` in `route.rs`).
//! - Sets `agent_doc_format=template` and `agent_doc_write=crdt` in frontmatter when
//!   neither `format`, `write_mode`, nor legacy `mode` is present.
//! - Scaffolds default `## Status` and `## Exchange` component sections when the
//!   document has none and format is `template`.
//! - Merges default component configuration into `.agent-doc/config.toml` (under the
//!   `[components]` section) if the document is template format.
//! - Registers the session→pane mapping using the pane's own PID (not the short-lived
//!   CLI process PID) via `sessions::register_with_pid`.
//! - Focuses the claimed pane via `tmux select-pane` (cross-window safe); warns but
//!   continues if the pane is not alive.
//! - Displays a 3-second tmux notification on the target pane.
//! - Appends a one-line entry to `.agent-doc/claims.log` for skill-side display.
//! - Lazy-starts the watch daemon via `watch::ensure_running` if not already running.
//! - `find_alive_window_in_registry` is pure (I/O-injected predicate) for unit testability.
//!
//! ## Agentic Contracts
//! - Claim is idempotent for an already-claimed live pane: re-claiming updates the
//!   registry entry and refocuses the pane without side-effects.
//! - **Binding invariant enforcement:** If the target pane is already claimed by a
//!   different session and the pane is alive, claim provisions a new pane for this
//!   document instead of erroring (SPEC §8.5: "never commandeer another document's
//!   pane"). Use `--force` to explicitly overwrite the existing claim.
//! - Stale claims (dead pane) are cleaned before the new claim is written; the
//!   caller never observes a registry with two entries for the same file.
//! - `agent_doc_format` and `agent_doc_write` are only set when ALL three of
//!   `format`, `write_mode`, and `mode` are absent — existing mode configuration
//!   is never overwritten.
//! - Component scaffolding is only applied when the document has no `status` or
//!   `exchange` component yet; existing components are preserved.
//! - **Snapshot initialization:** After registration, saves a snapshot with empty
//!   exchange content. Existing user text in the exchange becomes a diff on the
//!   next run, ensuring unresponded prompts are not absorbed into the baseline.
//! - `claims.log` failures are non-fatal: errors are logged to stderr and the claim
//!   itself succeeds.
//! - Watch daemon launch failure is non-fatal: a warning is emitted and claim succeeds.
//!
//! ## Evals
//! - find_alive_window_returns_first_alive_match: registry with three entries for same
//!   cwd where only `@3` is alive → returns `Some("@3")`.
//! - find_alive_window_skips_wrong_cwd: entry with matching window but wrong cwd is
//!   ignored; only the entry with the correct cwd is returned.
//! - find_alive_window_skips_empty_window: legacy entries with empty window field are
//!   skipped; entry with non-empty window is returned.
//! - find_alive_window_returns_none_when_all_dead: all windows report dead →
//!   returns `None`.
//! - find_alive_window_returns_none_for_empty_registry: empty registry → `None`.
//! - find_alive_window_returns_none_when_no_cwd_match: registry entries exist but none
//!   match the queried cwd → `None`.
//! - claim_generates_session_uuid: document without `agent_doc_session` frontmatter →
//!   after claim, file contains a valid UUID in frontmatter.
//! - claim_scaffolds_components: template document with no components → after claim,
//!   file contains `<!-- agent:status -->` and `<!-- agent:exchange -->` sections.
//! - claim_does_not_overwrite_existing_format: document with explicit `agent_doc_format`
//!   set → claim leaves the format field unchanged.
//! - strip_exchange_content: document with user text in exchange → returns document with
//!   empty exchange, preserving frontmatter and other components.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

use crate::{frontmatter, project_config, resync, route, sessions};

/// Outcome of the cross-session claim gate. Pure function output, separated
/// from side effects so it can be unit-tested without a live tmux server.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CrossSessionDecision {
    /// Pane's tmux session matches the configured project session — no gate fires.
    Accept,
    /// Configured project session no longer exists on the tmux server — there
    /// is nothing to "conflict with", so auto-accept the claim.
    AcceptStale,
    /// Cross-session claim explicitly allowed via `--force`.
    AcceptForce,
    /// Cross-session claim rejected — user must switch sessions or pass `--force`.
    Reject,
}

pub(crate) fn cross_session_decision(
    pane_session: &str,
    configured: &str,
    configured_alive: bool,
    force: bool,
) -> CrossSessionDecision {
    if pane_session == configured {
        return CrossSessionDecision::Accept;
    }
    if !configured_alive {
        return CrossSessionDecision::AcceptStale;
    }
    if force {
        return CrossSessionDecision::AcceptForce;
    }
    CrossSessionDecision::Reject
}

pub fn run(file: &Path, position: Option<&str>, pane: Option<&str>, window: Option<&str>, force: bool, isolate: bool) -> Result<()> {
    // --isolate: spawn a fresh Claude Code process in a new tmux window scoped to
    // the nearest git repo root for this document (#8jzg).
    if isolate {
        return run_isolate(file);
    }
    let _ = resync::prune(); // Clean stale entries before window resolution

    // Check for stale claims on this specific file and log if found
    validate_file_claim(file);

    // Canonicalize to handle CWD drift (e.g., when CWD is in a submodule)
    let file = &file.canonicalize().map_err(|_| {
        anyhow::anyhow!("file not found: {}", file.display())
    })?;

    // Validate --window if provided: if dead, fall back to a live project window
    let effective_window: Option<String> = if let Some(win) = window {
        let alive = is_window_alive(win);
        if alive {
            Some(win.to_string())
        } else {
            eprintln!("warning: window {} is dead, searching for alive window", win);
            find_alive_project_window()
        }
    } else {
        None
    };

    // Auto-scaffold empty files with full template BEFORE ensure_session.
    // ensure_session only writes agent_doc_session — it doesn't set agent_doc_format
    // or add components. Empty files need the full template in one step.
    {
        let raw = std::fs::read_to_string(file).unwrap_or_default();
        if raw.trim().is_empty() && file.extension() == Some(std::ffi::OsStr::new("md")) {
            eprintln!("[claim] auto-scaffolding empty file: {}", file.display());
            let session_id = uuid::Uuid::new_v4();
            let scaffold = format!(
                "---\nagent_doc_session: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Pending / Not Built\n\n<!-- agent:pending patch=replace -->\n<!-- /agent:pending -->\n",
                session_id
            );
            std::fs::write(file, &scaffold)?;
            crate::snapshot::save(file, &scaffold)?;
            crate::git::commit(file).ok(); // best-effort commit
        }
    }

    // Read file content and extract/generate session UUID (in memory only — no disk write yet)
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session(&content)?;

    let pane_id = if let Some(p) = pane {
        p.to_string() // Plugin-provided, authoritative
    } else if let Some(pos) = position {
        if let Some(ref win) = effective_window {
            // Scope position detection to the specified window
            sessions::pane_by_position_in_window(pos, win)?
        } else {
            sessions::pane_by_position(pos)?
        }
    } else {
        sessions::current_pane()?
    };

    // tmux_session frontmatter field is deprecated — no longer written on claim.
    // Session targeting now uses current_tmux_session() at route time.
    let tmux = sessions::Tmux::default_server();

    // Validate claiming pane is in the configured target session.
    // Reject cross-session claims unless --force is passed.
    if tmux.pane_alive(&pane_id) {
        let pane_tmux_session = tmux
            .cmd()
            .args(["display-message", "-t", &pane_id, "-p", "#{session_name}"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if let Some(configured) = crate::config::project_tmux_session()
            && !pane_tmux_session.is_empty()
            && pane_tmux_session != configured
        {
            let configured_alive = tmux.session_alive(&configured);
            match cross_session_decision(&pane_tmux_session, &configured, configured_alive, force) {
                CrossSessionDecision::Accept => {}
                CrossSessionDecision::AcceptStale => {
                    eprintln!(
                        "[claim] configured session '{}' is not alive — accepting claim on pane {} in session '{}' (stale-session auto-force)",
                        configured, pane_id, pane_tmux_session
                    );
                }
                CrossSessionDecision::AcceptForce => {
                    eprintln!(
                        "warning [--force]: registering cross-session pane {} (session '{}', configured '{}')",
                        pane_id, pane_tmux_session, configured
                    );
                }
                CrossSessionDecision::Reject => {
                    eprintln!(
                        "warning: pane {} is in tmux session '{}' but project session is '{}' — auto-forcing claim",
                        pane_id, pane_tmux_session, configured
                    );
                }
            }
        }
    }

    // Check if pane is already claimed by a different session.
    // Per the Binding invariant (SPEC §8.5): "document drives pane resolution —
    // find existing OR provision new, NEVER commandeer another document's pane."
    let file_str = file.to_string_lossy();
    {
        let registry = sessions::load().unwrap_or_default();
        for (existing_id, entry) in &registry {
            if entry.pane == pane_id && *existing_id != session_id
                && tmux.pane_alive(&pane_id)
            {
                if force {
                    eprintln!("warning: overwriting claim on pane {} (was {} → {})", pane_id, &existing_id[..8], &session_id[..8]);
                } else {
                    // Pane is occupied — provision a new pane instead of erroring.
                    // This enforces the Binding invariant: never commandeer.
                    eprintln!(
                        "[claim] pane {} is already claimed by {} (file: {}); provisioning a new pane",
                        pane_id, &existing_id[..8], entry.file
                    );
                    route::provision_pane(&tmux, file, &session_id, &file_str, None, &[]).map(|_| ())?;
                    return Ok(());
                }
            }
        }
    }

    // Pane validated — now safe to modify files
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("Generated session UUID: {}", session_id);
    }

    // Default to template+crdt if neither format nor write_mode nor legacy mode is set
    {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (fm, _) = frontmatter::parse(&content)?;
        if fm.format.is_none() && fm.write_mode.is_none() && fm.mode.is_none() {
            let updated = frontmatter::set_format_and_write(
                &content,
                frontmatter::AgentDocFormat::Template,
                frontmatter::AgentDocWrite::Crdt,
            )?;
            if updated != content {
                std::fs::write(file, &updated)
                    .with_context(|| format!("failed to write agent_doc_format/write to {}", file.display()))?;
                eprintln!("set agent_doc_format=template, agent_doc_write=crdt in {}", file.display());
            }
        }
    }

    // Scaffold default components for template documents
    {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (fm, _) = frontmatter::parse(&content)?;
        let resolved = fm.resolve_mode();
        let has_components = crate::component::parse(&content)
            .map(|comps| comps.iter().any(|c| c.name == "status" || c.name == "exchange"))
            .unwrap_or(false);
        if resolved.format == frontmatter::AgentDocFormat::Template && !has_components {
            let scaffolded = format!(
                "{}\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Pending / Not Built\n\n<!-- agent:pending patch=replace -->\n<!-- /agent:pending -->\n",
                content.trim_end()
            );
            std::fs::write(file, &scaffolded)
                .with_context(|| format!("failed to write component scaffolding to {}", file.display()))?;
            eprintln!("scaffolded default components in {}", file.display());
        }

        // Merge default components into .agent-doc/config.toml if template format
        if resolved.format == frontmatter::AgentDocFormat::Template {
            let mut proj_cfg = project_config::load_project();

            // Add default components if not already present
            proj_cfg.components.entry("exchange".to_string())
                .or_insert_with(|| project_config::ComponentConfig {
                    patch: "append".to_string(),
                    ..Default::default()
                });
            proj_cfg.components.entry("findings".to_string())
                .or_insert_with(|| project_config::ComponentConfig {
                    patch: "append".to_string(),
                    ..Default::default()
                });
            proj_cfg.components.entry("status".to_string())
                .or_insert_with(|| project_config::ComponentConfig {
                    patch: "replace".to_string(),
                    ..Default::default()
                });

            if let Err(e) = project_config::save_project(&proj_cfg) {
                eprintln!("warning: failed to save config with components: {}", e);
            } else {
                eprintln!("merged default components into .agent-doc/config.toml");
            }
        }
    }

    // Register session → pane (use the pane's actual PID, not our short-lived CLI PID)
    // Resolve cwd to the nearest git repo root for the document to avoid superproject
    // drift when claiming submodule-hosted documents (#tw4a).
    let pane_pid = sessions::pane_pid(&pane_id).unwrap_or(std::process::id());
    let resolved_cwd = crate::git::resolve_pane_cwd(file);
    eprintln!("[claim] using cwd={} for registry entry", resolved_cwd.display());
    sessions::register_with_pid_and_cwd(
        &session_id,
        &pane_id,
        &file_str,
        pane_pid,
        &resolved_cwd.to_string_lossy(),
    )?;

    // Focus the claimed pane (select-window + select-pane for cross-window support)
    if tmux.pane_alive(&pane_id) {
        if let Err(e) = tmux.select_pane(&pane_id) {
            eprintln!("warning: failed to focus pane {}: {}", pane_id, e);
        } else {
            eprintln!("focused pane {}", pane_id);
        }
    } else {
        eprintln!("warning: pane {} is not alive, skipping focus", pane_id);
    }

    // Show a brief notification on the target pane
    let msg = format!("Claimed {} (pane {})", file_str, pane_id);
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", &pane_id, "-d", "3000", &msg])
        .status()
    {
        eprintln!("warning: display-message failed: {}", e);
    }

    // Append to claims log so the skill can display it on next invocation
    let log_line = format!("Claimed {} for pane {}\n", file_str, pane_id);
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let log_path = project_root.join(".agent-doc/claims.log");
    if let Some(parent) = log_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("warning: failed to create claims log dir: {}", e);
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(mut f) => {
            if let Err(e) = write!(f, "{}", log_line) {
                eprintln!("warning: failed to write claims log: {}", e);
            }
        }
        Err(e) => eprintln!("warning: failed to open claims log: {}", e),
    }

    eprintln!(
        "Claimed {} for pane {} (session {})",
        file.display(),
        pane_id,
        &session_id[..8]
    );

    // Ensure the document has a snapshot + git baseline. If already initialized
    // (snapshot exists), this is a no-op.
    if let Err(e) = crate::snapshot::ensure_initialized(file) {
        eprintln!("warning: failed to initialize document: {}", e);
    }

    // Lazy-start watch daemon if not running
    match crate::watch::ensure_running() {
        Ok(true) => eprintln!("Watch daemon started."),
        Ok(false) => {} // already running
        Err(e) => eprintln!("warning: could not start watch daemon: {}", e),
    }

    Ok(())
}

/// Validate the existing claim for a file: if the claimed pane is dead, log and
/// remove it so the new claim can proceed cleanly. This handles the common case
/// of stale claims after a machine restart (tmux pane IDs are reassigned).
///
/// Called after `resync::prune()` which handles bulk dead-pane removal. This
/// function provides file-specific logging so the user sees *why* a re-claim
/// was needed rather than getting a silent no-op.
fn validate_file_claim(file: &Path) {
    let file_str = file.to_string_lossy();
    let registry_path = sessions::registry_path();
    let Ok(_lock) = sessions::RegistryLock::acquire(&registry_path) else {
        return;
    };
    let Ok(registry) = sessions::load() else {
        return;
    };

    let tmux = sessions::Tmux::default_server();

    // Find entries pointing to this file with dead panes
    let stale_keys: Vec<(String, String)> = registry
        .iter()
        .filter(|(_, entry)| {
            entry.file == file_str.as_ref() && !tmux.pane_alive(&entry.pane)
        })
        .map(|(k, e)| (k.clone(), e.pane.clone()))
        .collect();

    if stale_keys.is_empty() {
        return;
    }

    // Remove stale entries and save
    let mut registry = registry;
    for (key, pane) in &stale_keys {
        eprintln!(
            "stale claim: {} was bound to dead pane {}, replacing",
            file_str, pane
        );
        registry.remove(key);
    }
    let _ = sessions::save(&registry);
}

/// Strip user content from the exchange component, leaving just the markers.
/// This creates a snapshot baseline that treats existing user text as a diff.
pub(crate) fn strip_exchange_content(content: &str) -> String {
    if let Ok(components) = crate::component::parse(content)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        return exchange.replace_content(content, "\n");
    }
    content.to_string()
}

/// Check if a tmux window is alive by listing its panes.
fn is_window_alive(window: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["list-panes", "-t", window, "-F", "#{pane_id}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Search sessions.json for a live window belonging to the current project.
///
/// Iterates all entries in the session registry. For each entry whose `cwd`
/// matches the current working directory and has a non-empty `window` field,
/// checks if the window is alive. Returns the first alive match.
fn find_alive_project_window() -> Option<String> {
    let registry = sessions::load().ok()?;
    let cwd = std::env::current_dir().ok()?.to_string_lossy().to_string();
    find_alive_window_in_registry(&registry, &cwd, is_window_alive)
}

/// Spawn a fresh Claude Code session in a new tmux window, with cwd set to the
/// nearest git repo root for the document. This scopes CLAUDE.md, memory, and
/// skills to that repo rather than the superproject (#8jzg).
fn run_isolate(file: &Path) -> Result<()> {
    use std::process::Command;

    let file = &file.canonicalize().map_err(|_| {
        anyhow::anyhow!("file not found: {}", file.display())
    })?;

    // Resolve nearest git root for the document
    let cwd = crate::git::resolve_pane_cwd(file);
    let file_str = file.to_string_lossy();

    eprintln!(
        "[claim --isolate] spawning Claude in new window: cwd={} file={}",
        cwd.display(), file_str
    );

    // Resolve the agent-doc binary path (same binary currently running)
    let agent_doc_bin = std::env::current_exe()
        .unwrap_or_else(|_| "agent-doc".into())
        .to_string_lossy()
        .to_string();

    // Create a new tmux window running claude in the submodule root
    let shell_cmd = format!(
        "cd {} && {} start {}",
        cwd.display(),
        agent_doc_bin,
        file_str,
    );

    let status = Command::new("tmux")
        .args([
            "new-window",
            "-c", &cwd.to_string_lossy(),
            "-n", "agent-doc",
            &shell_cmd,
        ])
        .status()
        .context("failed to run tmux new-window")?;

    if !status.success() {
        anyhow::bail!("tmux new-window failed (exit {:?})", status.code());
    }

    eprintln!(
        "[claim --isolate] new window spawned with cwd={}",
        cwd.display()
    );
    Ok(())
}

/// Pure logic for finding an alive window in a registry.
/// Separated from I/O for testability.
fn find_alive_window_in_registry(
    registry: &sessions::SessionRegistry,
    cwd: &str,
    check_alive: impl Fn(&str) -> bool,
) -> Option<String> {
    for entry in registry.values() {
        if entry.cwd != cwd || entry.window.is_empty() {
            continue;
        }
        if check_alive(&entry.window) {
            eprintln!("found alive window {} from registry", entry.window);
            return Some(entry.window.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{SessionEntry, SessionRegistry};

    fn make_entry(cwd: &str, window: &str) -> SessionEntry {
        SessionEntry {
            pane: "%0".to_string(),
            pid: 1,
            cwd: cwd.to_string(),
            started: "2026-01-01".to_string(),
            file: "test.md".to_string(),
            window: window.to_string(),
        }
    }

    #[test]
    fn find_alive_window_returns_first_alive_match() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/project", "@1"));
        registry.insert("s2".into(), make_entry("/project", "@2"));
        registry.insert("s3".into(), make_entry("/project", "@3"));

        // @1 dead, @2 alive, @3 alive → returns @2 or @3 (HashMap order)
        // Use deterministic check: only @3 is alive
        let result = find_alive_window_in_registry(&registry, "/project", |w| w == "@3");
        assert_eq!(result, Some("@3".to_string()));
    }

    #[test]
    fn find_alive_window_skips_wrong_cwd() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/other-project", "@5"));
        registry.insert("s2".into(), make_entry("/project", "@6"));

        let result = find_alive_window_in_registry(&registry, "/project", |w| w == "@5" || w == "@6");
        assert_eq!(result, Some("@6".to_string()));
    }

    #[test]
    fn find_alive_window_skips_empty_window() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/project", "")); // legacy entry
        registry.insert("s2".into(), make_entry("/project", "@7"));

        let result = find_alive_window_in_registry(&registry, "/project", |_| true);
        assert_eq!(result, Some("@7".to_string()));
    }

    #[test]
    fn find_alive_window_returns_none_when_all_dead() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/project", "@1"));
        registry.insert("s2".into(), make_entry("/project", "@2"));

        let result = find_alive_window_in_registry(&registry, "/project", |_| false);
        assert_eq!(result, None);
    }

    #[test]
    fn find_alive_window_returns_none_for_empty_registry() {
        let registry = SessionRegistry::new();
        let result = find_alive_window_in_registry(&registry, "/project", |_| true);
        assert_eq!(result, None);
    }

    #[test]
    fn find_alive_window_returns_none_when_no_cwd_match() {
        let mut registry = SessionRegistry::new();
        registry.insert("s1".into(), make_entry("/other", "@1"));

        let result = find_alive_window_in_registry(&registry, "/project", |_| true);
        assert_eq!(result, None);
    }

    #[test]
    fn strip_exchange_content_removes_user_text() {
        let content = "---\nagent_doc_session: abc\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nUser prompt here.\n<!-- /agent:exchange -->\n";
        let result = strip_exchange_content(content);
        assert!(result.contains("<!-- agent:exchange"));
        assert!(!result.contains("User prompt here."));
    }

    #[test]
    fn strip_exchange_content_preserves_no_exchange() {
        let content = "---\nagent_doc_session: abc\n---\n\nJust text.\n";
        let result = strip_exchange_content(content);
        assert_eq!(result, content);
    }

    #[test]
    fn cross_session_accept_when_pane_matches_configured() {
        let d = cross_session_decision("0", "0", true, false);
        assert_eq!(d, CrossSessionDecision::Accept);
    }

    #[test]
    fn cross_session_accept_stale_when_configured_dead() {
        // Stale session (e.g. post-reboot): auto-accept without requiring --force.
        let d = cross_session_decision("claude", "0", false, false);
        assert_eq!(d, CrossSessionDecision::AcceptStale);
    }

    #[test]
    fn cross_session_accept_stale_takes_precedence_over_force() {
        // If configured is dead, prefer the "stale" classification so the warning
        // message reflects reality — not a cross-session override.
        let d = cross_session_decision("claude", "0", false, true);
        assert_eq!(d, CrossSessionDecision::AcceptStale);
    }

    #[test]
    fn cross_session_reject_when_configured_alive_and_no_force() {
        let d = cross_session_decision("claude", "0", true, false);
        assert_eq!(d, CrossSessionDecision::Reject);
    }

    #[test]
    fn cross_session_accept_force_when_configured_alive_and_force() {
        let d = cross_session_decision("claude", "0", true, true);
        assert_eq!(d, CrossSessionDecision::AcceptForce);
    }
}

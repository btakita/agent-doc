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
//! - Sets `agent_doc_format=template` and `agent_doc_write=crdt` in frontmatter when
//!   neither `format`, `write_mode`, nor legacy `mode` is present.
//! - Scaffolds default `## Status` and `## Exchange` component sections when the
//!   document has none and format is `template`.
//! - Creates `.agent-doc/components.toml` with default per-component patch modes if
//!   the file does not yet exist.
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
//! - **Registry protection:** If the target pane is already claimed by a different
//!   session and the pane is alive, claim refuses unless `--force` is passed. This
//!   prevents silent corruption when position detection falls back to the wrong pane.
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

use crate::{frontmatter, resync, sessions};

pub fn run(file: &Path, position: Option<&str>, pane: Option<&str>, window: Option<&str>, force: bool) -> Result<()> {
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

    // Validate pane BEFORE any file modifications — atomic claim semantics.
    // If the pane is already claimed by a different session, bail without orphaning
    // file changes (UUID, format, scaffolding).
    let file_str = file.to_string_lossy();
    {
        let registry = sessions::load().unwrap_or_default();
        for (existing_id, entry) in &registry {
            if entry.pane == pane_id && *existing_id != session_id
                && tmux.pane_alive(&pane_id)
            {
                if !force {
                    anyhow::bail!(
                        "pane {} is already claimed by {} (file: {}). Use --force to overwrite.",
                        pane_id, &existing_id[..8], entry.file
                    );
                }
                eprintln!("warning: overwriting claim on pane {} (was {} → {})", pane_id, &existing_id[..8], &session_id[..8]);
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

        // Create default .agent-doc/components.toml if it doesn't exist
        if resolved.format == frontmatter::AgentDocFormat::Template {
            let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
            let project_root = crate::snapshot::find_project_root(&canonical)
                .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
            let components_toml = project_root.join(".agent-doc/components.toml");
            if !components_toml.exists() {
                if let Some(parent) = components_toml.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    eprintln!("warning: failed to create .agent-doc dir: {}", e);
                }
                let default_config = "[exchange]\nmode = \"append\"\n\n[findings]\nmode = \"append\"\n\n[status]\nmode = \"replace\"\n";
                match std::fs::write(&components_toml, default_config) {
                    Ok(()) => eprintln!("created {}", components_toml.display()),
                    Err(e) => eprintln!("warning: failed to create components.toml: {}", e),
                }
            }
        }
    }

    // Register session → pane (use the pane's actual PID, not our short-lived CLI PID)
    let pane_pid = sessions::pane_pid(&pane_id).unwrap_or(std::process::id());
    sessions::register_with_pid(&session_id, &pane_id, &file_str, pane_pid)?;

    // Log if pane is in a different session than configured — but do NOT
    // auto-update config.toml. The configured session is the source of truth;
    // overwriting it causes cascading session migration bugs.
    if tmux.pane_alive(&pane_id) {
        let pane_session = tmux
            .cmd()
            .args(["display-message", "-t", &pane_id, "-p", "#{session_name}"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if !pane_session.is_empty() {
            let configured = crate::config::project_tmux_session();
            if configured.as_deref() != Some(&pane_session) {
                eprintln!(
                    "note: pane {} is in session '{}' but config says '{}' — config unchanged",
                    pane_id, pane_session, configured.as_deref().unwrap_or("(none)")
                );
            }
        }
    }

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
}

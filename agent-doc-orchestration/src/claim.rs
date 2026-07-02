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
//!   mismatches fail closed while the configured session is still alive; stale
//!   configured sessions are auto-accepted, and explicit `--force` is required to
//!   override a live cross-session mismatch. `--force` additionally overrides the
//!   binding invariant (commandeering another document's pane). This prevents the
//!   stash/rescue routing dance that fires whenever a document's pane is in the
//!   wrong session (`pane_session != target_session` in `route.rs`).
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
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;

use agent_doc_controller::claim::{
    CrossSessionDecision, cross_session_decision_with_lease, cross_session_reject_marker,
};
use agent_doc_document::claim_scaffold::{
    default_format_and_write_content, merge_default_template_component_config,
    render_empty_template_scaffold, scaffold_default_template_components,
    should_scaffold_empty_markdown, uses_template_format,
};
use agent_doc_frontmatter::frontmatter;
use agent_doc_supervisor::claim_binding::{
    ClaimRegistryEntry, claimed_session_label, find_alive_window_in_registry,
    registry_entry_matches_claimed_document,
};

use crate::{resync, route, sessions};
use agent_doc_project_config_io as project_config_io;

fn enforce_cross_session_claim(
    file: &Path,
    pane_id: &str,
    pane_tmux_session: &str,
    configured: &str,
    decision: CrossSessionDecision,
) -> Result<()> {
    match decision {
        CrossSessionDecision::Accept => Ok(()),
        CrossSessionDecision::AcceptStale => {
            eprintln!(
                "[claim] configured session '{}' is not alive — accepting claim on pane {} in session '{}' (stale-session auto-force)",
                configured, pane_id, pane_tmux_session
            );
            Ok(())
        }
        CrossSessionDecision::AcceptForce => {
            eprintln!(
                "warning [--force]: registering cross-session pane {} (session '{}', configured '{}')",
                pane_id, pane_tmux_session, configured
            );
            Ok(())
        }
        CrossSessionDecision::Reject => {
            // Structured signal first so plugins can branch on the reject and
            // offer Force claim / Switch project session / Cancel instead of
            // rendering the raw exit-1 bail text. Human message preserved below.
            let marker = cross_session_reject_marker(pane_id, pane_tmux_session, configured);
            eprintln!("{}", marker);
            // #x9ds: the structured signal above is stderr-only (the plugin branch
            // channel) and invisible to the ops.log gate-verify scan. Also record
            // it to ops.log so the #4wxr reject behavior is provable from ops.log
            // when driven live (claim from a pane in a non-configured session).
            crate::ops_log::log_op(file, &marker);
            anyhow::bail!(
                "pane {} is in tmux session '{}' but project session is '{}'; switch to the configured session or pass --force",
                pane_id,
                pane_tmux_session,
                configured
            )
        }
    }
}

pub fn run(
    file: &Path,
    position: Option<&str>,
    pane: Option<&str>,
    window: Option<&str>,
    force: bool,
    isolate: bool,
) -> Result<()> {
    // --isolate: spawn a fresh Claude Code process in a new tmux window scoped to
    // the nearest git repo root for this document (#8jzg).
    if isolate {
        return run_isolate(file);
    }
    let _ = resync::prune(); // Clean stale entries before window resolution

    // Check for stale claims on this specific file and log if found
    validate_file_claim(file);

    // Canonicalize to handle CWD drift (e.g., when CWD is in a submodule)
    let file = &file
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("file not found: {}", file.display()))?;

    // Validate --window if provided: if dead, fall back to a live project window
    let effective_window: Option<String> = if let Some(win) = window {
        let alive = is_window_alive(win);
        if alive {
            Some(win.to_string())
        } else {
            eprintln!(
                "warning: window {} is dead, searching for alive window",
                win
            );
            find_alive_project_window()
        }
    } else {
        None
    };

    // Resolve the claiming pane and reject a cross-session claim BEFORE any file
    // mutation. `#claim-validate-before-scaffold`: the empty-file auto-scaffold
    // below writes + commits the document, so a cross-session reject that only
    // fired afterwards left a committed `agent-doc(<doc>)` scaffold for a file the
    // operator never opted into (JB repro: an empty `test.md` was scaffolded and
    // committed, then the claim failed with "pane %N is in tmux session '1' but
    // project session is '0'"). Pane validation needs only the pane id and the
    // configured session, so run it first and fail closed before the scaffold
    // touches disk. The `// Pane validated — now safe to modify files` invariant
    // below applies to the auto-scaffold too.
    let tmux = tmux_router::Tmux::default_server();
    let pane_id = if let Some(p) = pane {
        p.to_string() // Plugin-provided, authoritative
    } else if let Some(pos) = position {
        if let Some(ref win) = effective_window {
            // Scope position detection to the specified window
            agent_doc_tmux_io::pane_by_position_in_window(&tmux, pos, win)?
        } else {
            agent_doc_tmux_io::pane_by_position(&tmux, pos)?
        }
    } else {
        agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux)
            .context("failed to query current tmux pane")?
    };

    // tmux_session frontmatter field is deprecated — no longer written on claim.
    // Session targeting now uses current_tmux_session() at route time.
    // Validate claiming pane is in the configured target session.
    // Reject cross-session claims unless --force is passed.
    if tmux.pane_alive(&pane_id) {
        let pane_tmux_session =
            agent_doc_tmux_io::target_session_name(&tmux, &pane_id).unwrap_or_default();
        if let Some(configured) = project_config_io::project_tmux_session()
            && !pane_tmux_session.is_empty()
            && pane_tmux_session != configured
        {
            let configured_alive = tmux.session_alive(&configured);
            // `#xdocsuper0`: before an `AcceptStale` auto-force can commandeer
            // this document, consult its supervisor lease. If a live foreign
            // supervisor still holds a fresh lease on the document, refuse the
            // auto-reclaim (require explicit `--force`) so two supervisors never
            // own one document. The lease lookup is keyed by the canonical
            // document path (the controller's `document_id`); errors / absent
            // state degrade to `false` (no guard fired).
            let fresh_foreign_lease = agent_doc_project_root_io::project_root_containing(file)
                .map(|project_root| {
                    crate::project_controller::fresh_foreign_supervisor_lease_holds_document(
                        &project_root,
                        &file.to_string_lossy(),
                        std::process::id(),
                        crate::project_controller::SUPERVISOR_LEASE_GUARD_STALE_AFTER,
                    )
                })
                .unwrap_or(false);
            enforce_cross_session_claim(
                file,
                &pane_id,
                &pane_tmux_session,
                &configured,
                cross_session_decision_with_lease(
                    &pane_tmux_session,
                    &configured,
                    configured_alive,
                    force,
                    fresh_foreign_lease,
                ),
            )?;
        }
    }

    // Auto-scaffold empty files with full template BEFORE ensure_session.
    // ensure_session only writes agent_doc_session — it doesn't set agent_doc_format
    // or add components. Empty files need the full template in one step.
    {
        let raw = std::fs::read_to_string(file).unwrap_or_default();
        let extension = file.extension().and_then(std::ffi::OsStr::to_str);
        if should_scaffold_empty_markdown(&raw, extension) {
            eprintln!("[claim] auto-scaffolding empty file: {}", file.display());
            let session_id = uuid::Uuid::new_v4();
            let scaffold = render_empty_template_scaffold(&session_id.to_string());
            std::fs::write(file, &scaffold)?;
            agent_doc_snapshot_io::save(file, &scaffold, crate::ops_log::log_op)?;
            crate::git::commit(file).ok(); // best-effort commit
        }
    }

    // Read file content and extract/generate session UUID (in memory only — no disk write yet)
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (updated_content, session_id) = frontmatter::ensure_session(&content)?;

    // Pane id, tmux handle, and the cross-session guard were resolved above,
    // before the auto-scaffold (`#claim-validate-before-scaffold`).

    // Check if pane is already claimed by a different session.
    // Per the Binding invariant (SPEC §8.5): "document drives pane resolution —
    // find existing OR provision new, NEVER commandeer another document's pane."
    let file_str = file.to_string_lossy();
    {
        let registry = agent_doc_session_registry_io::load().unwrap_or_default();
        for (registry_key, entry) in &registry {
            let same_document = registry_entry_matches_claimed_document(
                file,
                &session_id,
                ClaimRegistryEntry {
                    registry_key,
                    session_id: &entry.session_id,
                    file: &entry.file,
                    cwd: &entry.cwd,
                    window: &entry.window,
                },
                agent_doc_git_io::dirs::resolve_canonical_or_absolute_file_path,
            );
            if entry.pane == pane_id && !same_document && tmux.pane_alive(&pane_id) {
                let existing_label = claimed_session_label(ClaimRegistryEntry {
                    registry_key,
                    session_id: &entry.session_id,
                    file: &entry.file,
                    cwd: &entry.cwd,
                    window: &entry.window,
                });
                if force {
                    eprintln!(
                        "warning: overwriting claim on pane {} (was {} → {})",
                        pane_id,
                        existing_label,
                        &session_id[..8]
                    );
                } else {
                    // Pane is occupied — provision a new pane instead of erroring.
                    // This enforces the Binding invariant: never commandeer.
                    eprintln!(
                        "[claim] pane {} is already claimed by {} (file: {}); provisioning a new pane",
                        pane_id, existing_label, entry.file
                    );
                    route::provision_pane(&tmux, file, &session_id, &file_str, None, &[])
                        .map(|_| ())?;
                    return Ok(());
                }
            }
        }
    }

    // Cross-root binding guard (SPEC §8.5): the calling root's `sessions.json`
    // only records documents rooted under it, so the registry loop above cannot
    // see a pane owned by a document rooted in another project/submodule. Inspect
    // the pane's live process tree directly: if it runs an agent-doc/codex owner
    // session for a different document, never commandeer it — provision a new
    // pane. Without this, a new document claimed from inside another document's
    // live pane (e.g. a submodule Codex session) aliases onto that pane and no
    // real pane for the new document ever appears.
    if !force
        && tmux.pane_alive(&pane_id)
        && crate::sync::pane_runs_other_document_owner(&tmux, &pane_id, file)
    {
        eprintln!(
            "[claim] pane {} runs a live agent-doc/codex session for another document; provisioning a new pane instead of commandeering it",
            pane_id
        );
        route::provision_pane(&tmux, file, &session_id, &file_str, None, &[]).map(|_| ())?;
        return Ok(());
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
        if let Some(updated) = default_format_and_write_content(&content)? {
            std::fs::write(file, &updated).with_context(|| {
                format!(
                    "failed to write agent_doc_format/write to {}",
                    file.display()
                )
            })?;
            eprintln!(
                "set agent_doc_format=template, agent_doc_write=crdt in {}",
                file.display()
            );
        }
    }

    // Scaffold default components for template documents
    {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let is_template = uses_template_format(&content)?;
        if let Some(scaffolded) = scaffold_default_template_components(&content)? {
            std::fs::write(file, &scaffolded).with_context(|| {
                format!(
                    "failed to write component scaffolding to {}",
                    file.display()
                )
            })?;
            eprintln!("scaffolded default components in {}", file.display());
        }

        // Merge default components into .agent-doc/config.toml if template format
        if is_template {
            let mut proj_cfg = project_config_io::load_project();

            merge_default_template_component_config(&mut proj_cfg);

            if let Err(e) = project_config_io::save_project(&proj_cfg) {
                eprintln!("warning: failed to save config with components: {}", e);
            } else {
                eprintln!("merged default components into .agent-doc/config.toml");
            }
        }
    }

    // Register session → pane (use the pane's actual PID, not our short-lived CLI PID)
    // Resolve cwd to the nearest git repo root for the document to avoid superproject
    // drift when claiming submodule-hosted documents (#tw4a).
    let pane_pid = agent_doc_tmux_io::pane_pid(&tmux, &pane_id).unwrap_or_else(std::process::id);
    let resolved_cwd = agent_doc_git_io::dirs::resolve_pane_cwd(file);
    eprintln!(
        "[claim] using cwd={} for registry entry",
        resolved_cwd.display()
    );
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
    if let Err(e) = agent_doc_tmux_io::show_message(&tmux, &pane_id, "3000", &msg) {
        eprintln!("warning: display-message failed: {}", e);
    }

    // Append to claims log so the skill can display it on next invocation
    let log_line = format!("Claimed {} for pane {}\n", file_str, pane_id);
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
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
    if let Err(e) = agent_doc_workflow_io::document_init::ensure_initialized(
        file,
        crate::git::commit,
        crate::ops_log::log_op,
    ) {
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
    let registry_path = agent_doc_session_registry_io::registry_path();
    let Ok(_lock) = tmux_router::RegistryLock::acquire(&registry_path) else {
        return;
    };
    let Ok(registry) = agent_doc_session_registry_io::load() else {
        return;
    };

    let tmux = tmux_router::Tmux::default_server();

    // Find entries pointing to this file with dead panes
    let stale_keys: Vec<(String, String)> = registry
        .iter()
        .filter(|(registry_key, entry)| {
            registry_entry_matches_claimed_document(
                file,
                "",
                ClaimRegistryEntry {
                    registry_key,
                    session_id: &entry.session_id,
                    file: &entry.file,
                    cwd: &entry.cwd,
                    window: &entry.window,
                },
                agent_doc_git_io::dirs::resolve_canonical_or_absolute_file_path,
            ) && !agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(
                &tmux,
                &entry.pane,
            )
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
            "stale claim: {} was bound to pane {} with no live agent, replacing",
            file_str, pane
        );
        registry.remove(key);
    }
    let _ = agent_doc_session_registry_io::save(&registry);
}

/// Check if a tmux window is alive by listing its panes.
fn is_window_alive(window: &str) -> bool {
    let tmux = tmux_router::Tmux::default_server();
    agent_doc_tmux_io::list_panes(&tmux, Some(window), "#{pane_id}").is_ok()
}

/// Search sessions.json for a live window belonging to the current project.
///
/// Iterates all entries in the session registry. For each entry whose `cwd`
/// matches the current working directory and has a non-empty `window` field,
/// checks if the window is alive. Returns the first alive match.
fn find_alive_project_window() -> Option<String> {
    let registry = agent_doc_session_registry_io::load().ok()?;
    let cwd = std::env::current_dir().ok()?.to_string_lossy().to_string();
    let result = find_alive_window_in_registry(
        registry
            .iter()
            .map(|(registry_key, entry)| ClaimRegistryEntry {
                registry_key,
                session_id: &entry.session_id,
                file: &entry.file,
                cwd: &entry.cwd,
                window: &entry.window,
            }),
        &cwd,
        is_window_alive,
    );
    if let Some(window) = &result {
        eprintln!("found alive window {} from registry", window);
    }
    result
}

/// Spawn a fresh Claude Code session in a new tmux window, with cwd set to the
/// nearest git repo root for the document. This scopes CLAUDE.md, memory, and
/// skills to that repo rather than the superproject (#8jzg).
fn run_isolate(file: &Path) -> Result<()> {
    let file = &file
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("file not found: {}", file.display()))?;

    // Resolve nearest git root for the document
    let cwd = agent_doc_git_io::dirs::resolve_pane_cwd(file);
    let file_str = file.to_string_lossy();

    eprintln!(
        "[claim --isolate] spawning Claude in new window: cwd={} file={}",
        cwd.display(),
        file_str
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

    let tmux = tmux_router::Tmux::default_server();
    agent_doc_tmux_io::new_window_in_cwd(
        &tmux,
        cwd.to_string_lossy().as_ref(),
        "agent-doc",
        &shell_cmd,
    )
    .context("failed to run tmux new-window")?;

    eprintln!(
        "[claim --isolate] new window spawned with cwd={}",
        cwd.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_cross_session_claim_errors_on_reject() {
        let dir = tempfile::tempdir().unwrap();
        // find_project_root walks up for an `.agent-doc/` dir to resolve ops.log.
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "x").unwrap();
        let err =
            enforce_cross_session_claim(&doc, "%12", "claude", "0", CrossSessionDecision::Reject)
                .expect_err("reject should fail closed");
        assert_eq!(
            err.to_string(),
            "pane %12 is in tmux session 'claude' but project session is '0'; switch to the configured session or pass --force"
        );
        // #x9ds: the reject marker is recorded to ops.log (not just stderr) so the
        // #4wxr behavior is provable from the ops.log gate-verify scan.
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains(
                "[claim] cross-session-reject pane_id=%12 pane_session=claude configured=0"
            ),
            "reject marker should reach ops.log: {log}"
        );
    }
}

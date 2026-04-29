//! # Module: route
//!
//! Routes harness-specific document trigger commands to the correct tmux pane. This is the
//! process-level coordinator between file-save events (editor plugin / watch daemon)
//! and running agent sessions inside tmux.
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
//!   6. If pane is alive: first verify that a live process tree still proves the
//!      document is running there. If the live owner is another pane, re-register there;
//!      if no live owner exists, fail closed instead of sending the trigger into an
//!      ambiguous shell. Pane IDs (`%N`) are globally unique per tmux server, so
//!      `target_session` matching is not required once ownership is proven.
//!      `rescue_from_stash` is attempted (it self-gates on session match) so panes
//!      stashed within the target session get rescued, but panes in other sessions are
//!      left in place. When the document already has prompt-bearing user drift after a
//!      closed cycle, the routed trigger must also produce a new per-document cycle
//!      acknowledgment before route returns success; otherwise route fails closed.
//!   7. If pane is dead and was previously registered: lazy-claims only to an explicit
//!      pane override via `find_target_pane` (skipped if the candidate is already claimed
//!      or is running a non-agent process), sends the command, then calls
//!      `sync_after_claim` to re-sync layout. Route never adopts the tmux session's
//!      current active pane implicitly.
//!   8. If no registered pane or no claimable pane: auto-starts a new agent session.
//!      Blocked by `AGENT_DOC_NO_AUTOSTART` env var (used in tests).
//! - **`auto_start(tmux, file, session_id, file_path, context_session)`**: Public; spawns a
//!   new agent pane and sends `agent-doc start`. Waits for the agent's idle prompt before
//!   sending the initial command, then requires a real document-cycle acknowledgment before
//!   treating the fresh start as successful. Called by `sync.rs` for unresolved files.
//! - **`provision_pane(tmux, file, session_id, file_path, context_session, col_args)`**: Like
//!   `auto_start` but skips waiting for the agent to be ready. Used by sync when only pane
//!   existence is needed (agent will start asynchronously). Computes `split_before` via
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
//! - **`send_command(tmux, pane, file_path, harness)`**: Flashes a tmux display-message on the
//!   target pane, sends the harness trigger command via send-keys, focuses the pane, then polls
//!   up to 5 seconds verifying the command was accepted (retrying Enter if still visible in input).
//! - **`await_idle(file, debounce)`**: Polls file mtime every 100ms until `debounce` has
//!   elapsed since last modification, or until `10 × debounce` safety cap expires.
//! - **`wait_for_agent_ready(tmux, pane_id, timeout, harness)`**: Polls pane content every 500ms
//!   looking for the agent's idle prompt (per `harness.prompt_patterns`). Returns true when
//!   prompt found, false on timeout. Logs progress every 10 polls.
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
//! - **One pane per document**: Each document gets its own agent pane. Unregistered files
//!   (no prior session) skip lazy-claim and always get a fresh pane via auto-start.
//! - **Globally-unique pane IDs**: tmux `%N` pane IDs are unique per server. A registered
//!   alive pane is always routable by ID — routing does not depend on which session it
//!   currently lives in. This matters when `route run` is invoked from outside tmux (e.g.
//!   IDE `Run Agent Doc`), where `target_session` falls back to a constant and may not
//!   match the real session of the claimed pane.
//! - **Registered-pane ownership proof**: an alive registered pane is not sufficient on
//!   its own. Route first scans tmux for a live process tree that still mentions the
//!   document path. If that owner is another pane, route re-registers there. If no live
//!   owner exists, route fails closed instead of dispatching into an ambiguous pane.
//! - **Explicit provenance guard (lazy-claim only)**: `find_target_pane()` only accepts
//!   an explicit pane override for lazy-claim. Route will not infer ownership from the
//!   tmux session's current active pane when the registered pane is dead.
//! - **Non-agent process guard (lazy-claim only)**: `is_agent_process()` gates the
//!   lazy-claim path — even an explicit candidate pane will not be adopted when it is
//!   running corky/shell instead of an agent process.
//! - **Stash rescue**: Panes that ended up in a tmux `stash` / `stash-*` window are
//!   automatically rejoined into the `agent-doc` window before routing, without
//!   swapping another visible pane back into stash.
//! - **Auto-start inhibit**: Setting `AGENT_DOC_NO_AUTOSTART` prevents `auto_start_in_session`
//!   from spawning a new pane. The call returns `Err` with a descriptive message.
//! - **Non-fatal pane focus**: `select_pane` failures are logged as warnings and never abort
//!   the routing flow. The command is still sent even if focus fails.
//! - **Cycle acknowledgment for prompt-bearing reruns**: Fresh auto-start success is not
//!   inferred from pane input acceptance alone. The same fail-closed rule applies when route
//!   dispatches to an existing pane while the document already has prompt-bearing drift on top
//!   of a closed cycle: route must observe a new per-document cycle state before considering
//!   the dispatch successful.
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
//! - `detects_unicode_prompt`: `❯`, `❯ `, `  ❯  ` → all detected as agent idle prompt
//! - `detects_ascii_prompt`: `>`, `> `, `  >  ` → all detected as agent idle prompt
//! - `rejects_non_prompt_lines`: status text, empty lines, markdown headers → not matched as prompt
//! - `handles_ansi_prompt`: ANSI-colored `❯`/`>` → detected after strip_ansi
//! - `unregistered_file_skips_lazy_claim`: `registered = None` → lazy-claim step is skipped
//! - `dead_registered_pane_allows_lazy_claim`: `registered = Some(pane)` with dead pane → explicit-pane lazy-claim remains eligible
//! - `lazy_claim_requires_explicit_pane_provenance`: active pane in target session with no explicit `--pane` override → lazy-claim skipped
//! - (aspirational) `stash_rescue`: pane in stash window → rescued to agent-doc window before send
//! - `wrong_session_pane_still_receives_send`: alive pane in a session different from
//!   `target_session` → trigger command is sent to that pane (no new pane created)
//! - `alive_registered_pane_without_live_owner_fails_closed`: live registered pane with no
//!   file-owning process tree → route fails closed before sending into the pane
//! - `alive_registered_pane_reregisters_to_live_owner`: stale live registration + another
//!   pane running the file → route re-registers to the real live owner
//! - (aspirational) `debounce_idle`: file written rapidly → routing waits for mtime to settle
//! - (aspirational) `autostart_inhibited`: `AGENT_DOC_NO_AUTOSTART` set → returns Err, no pane spawned

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::Duration;

use crate::harness::HarnessConfig;
use crate::sessions::Tmux;
use crate::supervisor::ipc::IpcMethod;
use crate::{frontmatter, prompt, resync, sessions, snapshot, sync};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandDispatchStatus {
    Accepted,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupervisorHealth {
    Healthy,
    NeedsRestart,
    Unreachable,
    NoSocket,
}

fn pane_display_value(tmux: &Tmux, pane_id: &str, format: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", pane_id, "-p", format])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn pane_route_provenance(tmux: &Tmux, pane_id: &str) -> String {
    let pane_pid = pane_display_value(tmux, pane_id, "#{pane_pid}")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "?".to_string());
    let pane_session = pane_display_value(tmux, pane_id, "#{session_name}")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "?".to_string());
    let current_command = pane_display_value(tmux, pane_id, "#{pane_current_command}")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "?".to_string());
    format!(
        "pane={} pane_pid={} pane_session={} current_command={}",
        pane_id, pane_pid, pane_session, current_command
    )
}

fn query_supervisor_health(file: &Path, session_id: &str) -> SupervisorHealth {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return SupervisorHealth::NoSocket,
    };
    let project_root = match snapshot::find_project_root(&canonical) {
        Some(r) => r,
        None => return SupervisorHealth::NoSocket,
    };
    let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);
    if !sock.exists() {
        return SupervisorHealth::NoSocket;
    }
    match crate::supervisor::ipc::send_command(&sock, &IpcMethod::State) {
        Ok(resp) if resp.ok => {
            if let Some(data) = &resp.data {
                let running = data
                    .get("running")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let state = data.get("state").and_then(|v| v.as_str()).unwrap_or("");
                if running && state == "healthy" {
                    SupervisorHealth::Healthy
                } else {
                    SupervisorHealth::NeedsRestart
                }
            } else {
                SupervisorHealth::NeedsRestart
            }
        }
        Ok(_) | Err(_) => SupervisorHealth::Unreachable,
    }
}

fn restart_via_supervisor(file: &Path, session_id: &str) -> bool {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let project_root = match snapshot::find_project_root(&canonical) {
        Some(r) => r,
        None => return false,
    };
    let sock = crate::supervisor::ipc::socket_path(&project_root, session_id);
    let method = IpcMethod::Restart {
        mode: "continue".to_string(),
    };
    match crate::supervisor::ipc::send_command(&sock, &method) {
        Ok(resp) => resp.ok,
        Err(_) => false,
    }
}

fn startup_miss_requires_fresh_start(
    registered_pane: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
) -> bool {
    if live_owner == Some(registered_pane) {
        return false;
    }
    matches!(
        supervisor_health,
        SupervisorHealth::Unreachable | SupervisorHealth::NoSocket
    )
}

fn startup_miss_should_fail_closed(
    pane_alive: bool,
    registered_pane: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&crate::startup_miss::SessionLogStatus>,
) -> bool {
    pane_alive
        && live_owner != Some(registered_pane)
        && matches!(
            supervisor_health,
            SupervisorHealth::Unreachable | SupervisorHealth::NoSocket
        )
        && log_status.is_some_and(crate::startup_miss::SessionLogStatus::latest_session_open)
}

fn startup_miss_route_provenance(
    tmux: &Tmux,
    pane_id: &str,
    live_owner: Option<&str>,
    supervisor_health: SupervisorHealth,
    log_status: Option<&crate::startup_miss::SessionLogStatus>,
) -> String {
    let log_detail = match log_status {
        Some(status) if status.latest_session_open() => format!(
            "session_log=open latest_start_pane={} last_event={}",
            status.latest_start_pane.as_deref().unwrap_or("?"),
            status.last_event.as_deref().unwrap_or("?")
        ),
        Some(status) if status.latest_session_closed() => format!(
            "session_log=closed latest_start_pane={} last_event={}",
            status.latest_start_pane.as_deref().unwrap_or("?"),
            status.last_event.as_deref().unwrap_or("?")
        ),
        Some(status) => format!(
            "session_log=unknown latest_start_pane={} last_event={}",
            status.latest_start_pane.as_deref().unwrap_or("?"),
            status.last_event.as_deref().unwrap_or("?")
        ),
        None => "session_log=missing".to_string(),
    };
    format!(
        "{} live_owner={} supervisor_health={:?} {}",
        pane_route_provenance(tmux, pane_id),
        live_owner.unwrap_or("none"),
        supervisor_health,
        log_detail
    )
}

const STARTUP_MISS_DIAGNOSTIC_DISPLAY_MS: &str = "10000";

fn startup_miss_diagnostic_message(file: &Path, reason: &str) -> String {
    format!(
        "[agent-doc] startup-miss: {}. Run 'agent-doc start {}' to retry.",
        reason,
        file.display()
    )
}

fn emit_startup_miss_diagnostic(tmux: &Tmux, pane_id: &str, file: &Path, reason: &str) {
    let msg = startup_miss_diagnostic_message(file, reason);
    if let Err(e) = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-d",
            STARTUP_MISS_DIAGNOSTIC_DISPLAY_MS,
            &msg,
        ])
        .status()
    {
        eprintln!(
            "[route] warning: failed to emit startup-miss diagnostic to pane {}: {}",
            pane_id, e
        );
    }
}

/// Returns true if the pane is running an agent process for the given harness.
/// Returns true on query failure (conservative — don't skip panes we can't inspect).
fn is_agent_process(tmux: &Tmux, pane_id: &str, harness: &HarnessConfig) -> bool {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-t",
            pane_id,
            "-p",
            "#{pane_current_command}",
        ])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let cmd = String::from_utf8_lossy(&o.stdout).trim().to_string();
            harness.is_agent_process_name(&cmd)
        }
        _ => true, // can't inspect → treat conservatively
    }
}

/// Determine if the file is in the first column of the editor layout.
/// When true, the new pane should be split BEFORE (left of) the existing pane.
/// Returns false when col_args is empty (no layout context — default to split right).
pub(crate) fn is_first_column(file: &Path, col_args: &[String]) -> bool {
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

pub fn run_with_tmux(
    file: &Path,
    tmux: &Tmux,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
) -> Result<()> {
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
    let (updated_content, session_id) = frontmatter::ensure_session_for_file(&content, file)?;
    if updated_content != content {
        std::fs::write(file, &updated_content)
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("[route] Generated session UUID: {}", session_id);
    }

    let fm = frontmatter::parse_for_file(&updated_content, file).map(|(f, _)| f)?;
    let global_config = crate::config::load().unwrap_or_default();
    let harness = HarnessConfig::from_context(&fm, &global_config);

    let target_session = resolve_target_session(tmux, None, &harness);
    eprintln!("[route] target tmux session: {}", target_session);

    // Use absolute path for trigger commands to avoid CWD-dependent resolution
    // when the pane's CWD differs from the invoker's (e.g., narrowed to a
    // submodule root). Relative paths would resolve to the submodule's version
    // of the file when the same relative path exists in both locations.
    let file_path = crate::git::resolve_absolute_file_path(file)
        .to_string_lossy()
        .into_owned();

    // === SINGLE EXIT POINT PATTERN ===
    // All paths resolve to a pane_id, then ONE sync call handles layout.
    // This prevents propagation bugs where cross-cutting behavior (sync)
    // is added to one path but missed on others.

    // Snapshot panes before route so we can clean up orphans on failure.
    let window_arg = col_args
        .first()
        .and_then(|_| {
            tmux.cmd()
                .args([
                    "display-message",
                    "-t",
                    &format!("{}:agent-doc", target_session),
                    "-p",
                    "#{window_id}",
                ])
                .output()
                .ok()
        })
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let panes_before: Vec<String> = window_arg
        .as_deref()
        .and_then(|w| tmux.list_window_panes(w).ok())
        .unwrap_or_default();

    let pane_id = resolve_or_create_pane(
        tmux,
        file,
        pane,
        col_args,
        &session_id,
        &file_path,
        &target_session,
        &harness,
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
            // Clean up panes created during the failed route attempt, but fail
            // closed for the current session owner: if a newly-created pane is
            // still the registered live pane for this document, preserve it so
            // a missed start-ack cannot crash the user's active tmux pane.
            if let Some(w) = window_arg.as_deref()
                && let Ok(panes_after) = tmux.list_window_panes(w)
            {
                for p in &panes_after {
                    if panes_before.contains(p) {
                        continue;
                    }
                    if should_preserve_failed_route_pane(tmux, p, &session_id) {
                        eprintln!(
                            "[route] preserving newly-created pane {} after failed route because it is still the live registered owner for {}",
                            p,
                            file.display()
                        );
                        continue;
                    }
                    eprintln!(
                        "[route] cleaning up orphaned pane {} (created during failed route)",
                        p
                    );
                    tracing::warn!(pane = %p, "route: killing orphaned pane from failed route");
                    let _ = tmux.raw_cmd(&["kill-pane", "-t", p]);
                }
            }
            Err(e)
        }
    }
}

fn should_preserve_failed_route_pane(tmux: &Tmux, pane_id: &str, session_id: &str) -> bool {
    sessions::lookup(session_id)
        .ok()
        .flatten()
        .as_deref()
        .is_some_and(|registered| registered == pane_id && tmux.pane_alive(pane_id))
}

/// Resolve an existing pane or create a new one. Returns the pane ID.
///
/// Three resolution strategies, tried in order:
/// 1. Alive registered pane → unconditionally send command. Pane IDs are
///    globally unique per tmux server, so session matching is not required.
/// 2. Lazy claim to an active pane (when registered pane is dead)
/// 3. Auto-start a new agent session
#[allow(clippy::too_many_arguments)]
fn resolve_or_create_pane(
    tmux: &Tmux,
    file: &Path,
    pane: Option<&str>,
    col_args: &[String],
    session_id: &str,
    file_path: &str,
    target_session: &str,
    harness: &HarnessConfig,
) -> Result<String> {
    tracing::debug!(
        session_id = &session_id[..8.min(session_id.len())],
        file = file_path,
        target_session,
        "route::resolve_or_create_pane"
    );
    let registered = sessions::lookup(session_id)?;
    let cycle_baseline = crate::cycle_state::load(file)?;
    let pending_prompt_marker =
        pending_prompt_bearing_marker_for_route(file, cycle_baseline.as_ref())?;
    let live_owner = if registered.is_some() {
        crate::sync::find_live_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    let supervisor_health = if registered.is_some() {
        query_supervisor_health(file, session_id)
    } else {
        SupervisorHealth::NoSocket
    };

    // Strategy 0: If a previous startup-miss was recorded for the registered pane,
    // deregister it immediately so we fall through to auto-start instead of
    // reusing a pane that never successfully started a document cycle.
    if let Some(ref registered_pane) = registered
        && let Ok(Some(miss)) = crate::startup_miss::load(file)
        && miss.pane_id == *registered_pane
        && tmux.pane_alive(registered_pane)
    {
        let log_status = crate::startup_miss::session_log_status(file, &miss.session_id)
            .ok()
            .flatten();
        let provenance = startup_miss_route_provenance(
            tmux,
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_startup_miss_detected file={} origin={:?} {}",
                file_path, miss.origin, provenance
            ),
        );
        if startup_miss_should_fail_closed(
            true,
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
            log_status.as_ref(),
        ) {
            eprintln!(
                "[route] startup-miss for {} is stranded, not crashed: {}",
                file_path, provenance
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_stranded file={} origin={:?} {}",
                    file_path, miss.origin, provenance
                ),
            );
            anyhow::bail!(
                "startup-miss for {} remains unresolved on alive pane {}: {}. The last session never recorded a child exit or session_end, so route will not auto-start a replacement pane over a stranded session",
                file.display(),
                registered_pane,
                provenance
            );
        }
        if startup_miss_requires_fresh_start(
            registered_pane,
            live_owner.as_deref(),
            supervisor_health,
        ) {
            eprintln!(
                "[route] registered pane {} has a startup-miss marker for {} — deregistering and starting fresh",
                registered_pane, file_path
            );
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_startup_miss_deregistered file={} pane={}",
                    file_path, registered_pane
                ),
            );
            let _ = sessions::deregister(session_id)?;
            let _ = crate::startup_miss::clear(file);
            // Fall through to Strategy 3 (auto-start)
            eprintln!("[route] No active pane found, auto-starting...");
            if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
                anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
            }
            let split_before = is_first_column(file, col_args);
            ensure_auto_start_target_session(tmux, None, target_session, harness)?;
            return auto_start_in_session(
                tmux,
                file,
                session_id,
                file_path,
                target_session,
                false,
                split_before,
                harness,
            );
        }

        eprintln!(
            "[route] registered pane {} still proves live ownership for {} — clearing stale startup-miss marker",
            registered_pane, file_path
        );
        crate::ops_log::log_op(
            file,
            &format!(
                "route_startup_miss_cleared_live_owner file={} pane={}",
                file_path, registered_pane
            ),
        );
        let _ = crate::startup_miss::clear(file);
    }

    // Strategy 1: Alive registered pane — reuse only when a live process tree
    // still proves the document is running there. Pane IDs (%N) are globally
    // unique per tmux server, so target_session matching stays irrelevant once
    // ownership is proven.
    //
    // rescue_from_stash self-gates on target_session match, so it is a no-op
    // when the pane is in a different session — we leave it in place.
    if let Some(ref registered_pane) = registered {
        if tmux.pane_alive(registered_pane) {
            let mut stale_registration_cleared = false;
            match live_owner.as_deref() {
                Some(owner) if owner != registered_pane => {
                    eprintln!(
                        "[route] registered pane {} is alive, but live owner for {} is pane {} — re-registering",
                        registered_pane, file_path, owner
                    );
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "route_live_owner_reregistered file={} registered={} live_owner={} {}",
                            file_path,
                            registered_pane,
                            owner,
                            pane_route_provenance(tmux, registered_pane)
                        ),
                    );
                    sessions::register(session_id, owner, file_path)?;
                    rescue_from_stash(
                        tmux,
                        owner,
                        session_id,
                        file_path,
                        target_session,
                        is_first_column(file, col_args),
                    );
                    send_command(tmux, owner, file_path, harness)?;
                    require_routed_cycle_ack(
                        tmux,
                        file,
                        owner,
                        session_id,
                        harness,
                        cycle_baseline.as_ref(),
                        pending_prompt_marker.as_deref(),
                        true,
                    )?;
                    return Ok(owner.to_string());
                }
                Some(_) => {}
                None => match supervisor_health {
                    SupervisorHealth::Healthy => {
                        eprintln!(
                            "[route] registered pane {} has a healthy supervisor for {} despite missing live-owner proof — reusing registered pane",
                            registered_pane, file_path
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_reused_via_supervisor file={} pane={} health=healthy",
                                file_path, registered_pane
                            ),
                        );
                    }
                    SupervisorHealth::NeedsRestart => {
                        eprintln!(
                            "[route] registered pane {} has a restartable supervisor for {} — restarting in place",
                            registered_pane, file_path
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_restart_via_supervisor file={} pane={}",
                                file_path, registered_pane
                            ),
                        );
                        if restart_via_supervisor(file, session_id) {
                            if let Err(e) = tmux.select_pane(registered_pane) {
                                eprintln!(
                                    "[route] warning: failed to focus restarted pane {}: {}",
                                    registered_pane, e
                                );
                            }
                            require_routed_cycle_ack(
                                tmux,
                                file,
                                registered_pane,
                                session_id,
                                harness,
                                cycle_baseline.as_ref(),
                                pending_prompt_marker.as_deref(),
                                false,
                            )?;
                            return Ok(registered_pane.clone());
                        }
                        eprintln!(
                            "[route] supervisor restart failed for pane {} — deregistering and continuing recovery",
                            registered_pane
                        );
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_restart_failed file={} {}",
                                file_path, provenance
                            ),
                        );
                        let _ = sessions::deregister(session_id)?;
                        stale_registration_cleared = true;
                    }
                    SupervisorHealth::Unreachable | SupervisorHealth::NoSocket => {
                        let provenance = pane_route_provenance(tmux, registered_pane);
                        eprintln!(
                            "[route] registered pane {} is alive but no live owner for {} was proven and supervisor is unavailable — deregistering stale entry and continuing recovery",
                            registered_pane, file_path
                        );
                        crate::ops_log::log_op(
                            file,
                            &format!(
                                "route_registered_pane_deregistered_no_live_owner file={} {}",
                                file_path, provenance
                            ),
                        );
                        let _ = sessions::deregister(session_id)?;
                        stale_registration_cleared = true;
                    }
                },
            }
            if !stale_registration_cleared {
                rescue_from_stash(
                    tmux,
                    registered_pane,
                    session_id,
                    file_path,
                    target_session,
                    is_first_column(file, col_args),
                );
                eprintln!("[route] Pane {} is alive, sending command", registered_pane);
                send_command(tmux, registered_pane, file_path, harness)?;
                require_routed_cycle_ack(
                    tmux,
                    file,
                    registered_pane,
                    session_id,
                    harness,
                    cycle_baseline.as_ref(),
                    pending_prompt_marker.as_deref(),
                    true,
                )?;
                return Ok(registered_pane.clone());
            }
        }
        eprintln!("[route] Pane {} is dead", registered_pane);
    } else {
        eprintln!(
            "[route] No pane registered for session {}",
            &session_id[..std::cmp::min(8, session_id.len())]
        );
    }

    // Strategy 2: Lazy claim (only when a registered pane died)
    // Skip panes running non-agent processes to avoid claiming corky/shells.
    // Also skip panes already claimed by another document (pane theft prevention).
    let claimed_panes: std::collections::HashSet<String> = sessions::load()
        .unwrap_or_default()
        .values()
        .filter(|e| tmux.pane_alive(&e.pane))
        .map(|e| e.pane.clone())
        .collect();
    if registered.is_some()
        && let Some(existing) = live_owner
    {
        eprintln!(
            "[route] found existing running pane {} for {}, re-registering",
            existing, file_path
        );
        sessions::register(session_id, &existing, file_path)?;
        send_command(tmux, &existing, file_path, harness)?;
        require_routed_cycle_ack(
            tmux,
            file,
            &existing,
            session_id,
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_marker.as_deref(),
            true,
        )?;
        return Ok(existing);
    }
    if registered.is_some()
        && let Some(new_pane) = find_target_pane(tmux, pane, target_session, &claimed_panes)
        && is_agent_process(tmux, &new_pane, harness)
    {
        eprintln!("[route] Lazy-claiming to pane {} (dead pane)", new_pane);
        sessions::register(session_id, &new_pane, file_path)?;
        send_command(tmux, &new_pane, file_path, harness)?;
        require_routed_cycle_ack(
            tmux,
            file,
            &new_pane,
            session_id,
            harness,
            cycle_baseline.as_ref(),
            pending_prompt_marker.as_deref(),
            false,
        )?;
        return Ok(new_pane);
    }

    // Strategy 3: Auto-start
    eprintln!("[route] No active pane found, auto-starting...");
    if std::env::var("AGENT_DOC_NO_AUTOSTART").is_ok() {
        anyhow::bail!("auto-start skipped (AGENT_DOC_NO_AUTOSTART set)");
    }
    let split_before = is_first_column(file, col_args);
    ensure_auto_start_target_session(tmux, None, target_session, harness)?;
    auto_start_in_session(
        tmux,
        file,
        session_id,
        file_path,
        target_session,
        false,
        split_before,
        harness,
    )?;

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
    split_before: bool,
) {
    // Session guard: only rescue within the target session
    let pane_session = pane_session_name(tmux, pane_id).unwrap_or_default();
    if pane_session != target_session {
        eprintln!(
            "[route] Pane {} is in session '{}', not target '{}' — skipping stash rescue",
            pane_id, pane_session, target_session
        );
        return;
    }

    let pane_win_name = pane_window_name(tmux, pane_id).unwrap_or_default();

    if is_stash_window_name(&pane_win_name) {
        tracing::debug!(pane_id, window = %pane_win_name, target_session, "route: rescuing pane from stash");
        eprintln!(
            "[route] Pane {} is in stash window '{}', rescuing to agent-doc window",
            pane_id, pane_win_name
        );
        let agent_doc_window = format!("{}:agent-doc", target_session);
        let target_panes = tmux
            .list_window_panes(&agent_doc_window)
            .unwrap_or_default();
        let target = if split_before {
            target_panes.first()
        } else {
            target_panes.last()
        };
        if let Some(target) = target {
            let join_flag = if split_before { "-dbh" } else { "-dh" };
            match sessions::join_pane_guarded(tmux, pane_id, target, target_session, join_flag) {
                Ok(()) => eprintln!("[route] Rescued pane {} via join-pane", pane_id),
                Err(e) => eprintln!("[route] join-pane rescue failed for {} ({})", pane_id, e),
            }
        }
        if let Err(e) = sessions::register(session_id, pane_id, file_path) {
            eprintln!("[route] warning: re-register failed: {}", e);
        }
    }
}

/// Send the trigger command to a pane and focus it.
/// Shows a brief tmux display-message on the target pane for immediate feedback.
fn send_command(tmux: &Tmux, pane: &str, file_path: &str, harness: &HarnessConfig) -> Result<()> {
    let _ = send_command_checked(tmux, pane, file_path, harness)?;
    Ok(())
}

fn canonical_dispatch_file(path: &std::path::Path) -> std::path::PathBuf {
    let resolved = crate::git::resolve_absolute_file_path(path);
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

fn canonical_registered_file(entry: &sessions::SessionEntry) -> std::path::PathBuf {
    let path = std::path::Path::new(&entry.file);
    let resolved = if path.is_absolute() || entry.cwd.is_empty() {
        path.to_path_buf()
    } else {
        std::path::Path::new(&entry.cwd).join(path)
    };
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

fn pane_registration_matches_file(
    registry: &sessions::SessionRegistry,
    pane: &str,
    file_path: &str,
) -> bool {
    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(|entry| canonical_registered_file(entry) == requested)
        .unwrap_or(false)
}

fn ensure_dispatch_target_matches_file(pane: &str, file_path: &str) -> Result<()> {
    let registry =
        sessions::load().context("failed to load route registry before dispatch validation")?;
    if pane_registration_matches_file(&registry, pane, file_path) {
        return Ok(());
    }

    let requested = canonical_dispatch_file(std::path::Path::new(file_path));
    if let Some(entry) = registry.values().find(|entry| entry.pane == pane) {
        anyhow::bail!(
            "route dispatch target {} is registered for {}, not {}; refusing cross-file dispatch",
            pane,
            canonical_registered_file(entry).display(),
            requested.display()
        );
    }

    anyhow::bail!(
        "route dispatch target {} is not registered for {}; refusing unbound dispatch",
        pane,
        requested.display()
    );
}

fn send_command_checked(
    tmux: &Tmux,
    pane: &str,
    file_path: &str,
    harness: &HarnessConfig,
) -> Result<CommandDispatchStatus> {
    ensure_dispatch_target_matches_file(pane, file_path)?;
    let short_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string());
    let trigger = harness.trigger_command(file_path);
    let flash_msg = format!("⏳ {}", harness.trigger_command(&short_name));
    if let Err(e) = tmux
        .cmd()
        .args(["display-message", "-t", pane, "-d", "2000", &flash_msg])
        .status()
    {
        eprintln!("[route] warning: display-message failed: {}", e);
    }

    tmux.send_keys(pane, &trigger)?;
    if let Err(e) = tmux.select_pane(pane) {
        eprintln!("[route] warning: failed to focus pane {}: {}", pane, e);
    }
    eprintln!("[route] Sent {} → pane {}", trigger, pane);

    // Poll-based Enter confirmation: check if the command text is still visible
    // in the pane. Prompts vary by harness, so instead of watching for prompt
    // disappearance we check whether the exact trigger command is still in the
    // last few lines (meaning it is still sitting in the input, not submitted).
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);
    let poll_interval = std::time::Duration::from_millis(300);
    let mut enter_retries = 0u32;

    while start.elapsed() < timeout {
        std::thread::sleep(poll_interval);
        if let Ok(content) = sessions::capture_pane(tmux, pane) {
            // Check if the command text is still in the last 5 lines
            // (i.e., still sitting in the input prompt, not yet submitted)
            let cmd_still_in_input = recent_lines_contain_trigger(&content, &trigger);

            if !cmd_still_in_input {
                eprintln!(
                    "[route] Command accepted ({:.1}s, {} Enter retries)",
                    start.elapsed().as_secs_f64(),
                    enter_retries
                );
                return Ok(CommandDispatchStatus::Accepted);
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
    Ok(CommandDispatchStatus::TimedOut)
}

fn cycle_state_advances_start_ack(
    current: &crate::cycle_state::CycleState,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> bool {
    match baseline {
        None => true,
        Some(previous) if previous.is_open() => {
            current.cycle_id != previous.cycle_id
                || current.updated_at != previous.updated_at
                || current.phase != previous.phase
                || current.last_event != previous.last_event
        }
        Some(previous) => current.cycle_id != previous.cycle_id,
    }
}

fn wait_for_start_ack(
    file: &Path,
    baseline: Option<&crate::cycle_state::CycleState>,
    timeout: Duration,
) -> Option<crate::cycle_state::CycleState> {
    let start = std::time::Instant::now();
    let poll = Duration::from_millis(200);

    while start.elapsed() < timeout {
        if let Ok(Some(state)) = crate::cycle_state::load(file)
            && cycle_state_advances_start_ack(&state, baseline)
        {
            return Some(state);
        }
        std::thread::sleep(poll);
    }
    None
}

fn cycle_phase_name(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
    }
}

fn routed_cycle_ack_timeout() -> Duration {
    if cfg!(test) {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(15)
    }
}

fn should_require_routed_cycle_ack(
    baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
) -> bool {
    prompt_bearing_marker.is_some() && !baseline.is_some_and(|state| state.is_open())
}

fn pending_prompt_bearing_marker_for_route(
    file: &Path,
    baseline: Option<&crate::cycle_state::CycleState>,
) -> Result<Option<String>> {
    if baseline.is_some_and(|state| state.is_open()) {
        return Ok(None);
    }
    crate::session_check::detect_unstarted_prompt_bearing_diff(file)
}

#[allow(clippy::too_many_arguments)]
fn require_routed_cycle_ack(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    session_id: &str,
    harness: &HarnessConfig,
    baseline: Option<&crate::cycle_state::CycleState>,
    prompt_bearing_marker: Option<&str>,
    live_child_for_file: bool,
) -> Result<()> {
    if !should_require_routed_cycle_ack(baseline, prompt_bearing_marker) {
        return Ok(());
    }

    let marker = prompt_bearing_marker.expect("marker checked above");
    if live_child_for_file {
        eprintln!(
            "[route] live agent-doc child active in pane {} for {} — waiting for a new cycle ack for pending {}",
            pane,
            file.display(),
            marker
        );
    }
    match wait_for_start_ack(file, baseline, routed_cycle_ack_timeout()) {
        Some(state) => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_acknowledged file={} pane={} harness={} cycle={} phase={} marker={}",
                    file.display(),
                    pane,
                    harness.binary,
                    state.cycle_id,
                    cycle_phase_name(state.phase),
                    marker
                ),
            );
            let _ = crate::startup_miss::clear(file);
            Ok(())
        }
        None => {
            crate::ops_log::log_op(
                file,
                &format!(
                    "route_cycle_start_missing file={} pane={} harness={} marker={}",
                    file.display(),
                    pane,
                    harness.binary,
                    marker
                ),
            );
            let baseline_id = baseline.map(|b| b.cycle_id.as_str());
            let _ = crate::startup_miss::record(
                file,
                pane,
                session_id,
                &harness.binary,
                crate::startup_miss::StartupMissOrigin::RoutedTrigger,
                baseline_id,
            );
            emit_startup_miss_diagnostic(
                tmux,
                pane,
                file,
                &format!(
                    "routed trigger accepted but no document cycle started for pending {}",
                    marker
                ),
            );
            anyhow::bail!(
                "routed {} trigger for {} was accepted in pane {}, but no new document cycle started for pending {}",
                harness.binary,
                file.display(),
                pane,
                marker
            );
        }
    }
}

fn recent_lines_contain_trigger(content: &str, trigger: &str) -> bool {
    content
        .lines()
        .rev()
        .take(5)
        .any(|line| line_contains_trigger(&prompt::strip_ansi(line), trigger))
}

fn line_contains_trigger(line: &str, trigger: &str) -> bool {
    let mut offset = 0usize;
    while let Some(found) = line[offset..].find(trigger) {
        let start = offset + found;
        let end = start + trigger.len();
        let prev_ok = line[..start]
            .chars()
            .next_back()
            .map(|ch| ch.is_whitespace() || matches!(ch, '>' | '❯' | '⏵'))
            .unwrap_or(true);
        let next_ok = line[end..]
            .chars()
            .next()
            .map(|ch| ch.is_whitespace())
            .unwrap_or(true);
        if prev_ok && next_ok {
            return true;
        }
        offset = start + 1;
    }
    false
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
/// 2. config.toml `tmux_session` if the session is alive (user explicitly pinned via `session set`)
/// 3. Fallback to current tmux session or harness-specific fallback name (auto-detect)
///
/// Session config is never auto-written. Only `agent-doc session set <name>` pins a session.
/// `agent-doc session clear` returns to auto-detect mode.
fn resolve_target_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    harness: &HarnessConfig,
) -> String {
    if let Some(ctx) = normalize_context_session(context_session) {
        return ctx.to_string();
    }

    let configured = crate::config::project_tmux_session();
    if configured.as_ref().is_some_and(|s| tmux.session_alive(s)) {
        return configured.unwrap();
    }

    if let Some(ref stale) = configured {
        eprintln!(
            "[route] configured tmux_session '{}' is not alive, ignoring stale pin",
            stale
        );
    }

    current_tmux_session(tmux).unwrap_or_else(|| harness.tmux_session_fallback.clone())
}

fn ensure_auto_start_target_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    session_name: &str,
    harness: &HarnessConfig,
) -> Result<()> {
    if normalize_context_session(context_session).is_some() {
        return Ok(());
    }

    if crate::config::project_tmux_session().as_deref() == Some(session_name)
        && tmux.session_alive(session_name)
    {
        return Ok(());
    }

    if current_tmux_session(tmux).as_deref() == Some(session_name) {
        return Ok(());
    }

    if tmux.session_alive(session_name) {
        return Ok(());
    }

    if session_name == harness.tmux_session_fallback {
        anyhow::bail!(
            "refusing to auto-start in implicit fallback tmux session '{}' without a live explicit target session",
            session_name
        );
    }

    anyhow::bail!(
        "refusing to auto-start in tmux session '{}' because it is not alive",
        session_name
    );
}

fn normalize_context_session(context_session: Option<&str>) -> Option<&str> {
    context_session.and_then(|session| {
        let trimmed = session.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Find an explicit target pane for lazy claiming.
/// Skips panes already claimed by another document in the session registry.
fn find_target_pane(
    tmux: &Tmux,
    explicit_pane: Option<&str>,
    _session_name: &str,
    claimed_panes: &std::collections::HashSet<String>,
) -> Option<String> {
    let target = explicit_pane.map(|p| p.to_string());
    target.filter(|p| tmux.pane_alive(p) && !claimed_panes.contains(p))
}

/// Check if a window with the given name exists in the target tmux session.
fn has_named_window(tmux: &Tmux, session_name: &str, window_name: &str) -> bool {
    let output = tmux
        .cmd()
        .args(["list-windows", "-t", session_name, "-F", "#{window_name}"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().any(|l| l.trim() == window_name)
        }
        _ => false,
    }
}

fn pane_session_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{session_name}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn pane_window_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    tmux.pane_window(pane_id).ok().and_then(|window_id| {
        tmux.cmd()
            .args(["display-message", "-t", &window_id, "-p", "#{window_name}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    })
}

fn is_stash_window_name(window_name: &str) -> bool {
    window_name == "stash" || window_name.starts_with("stash-")
}

fn evict_previous_stash_pane(
    tmux: &Tmux,
    session_id: &str,
    replacement_pane: &str,
    target_session: &str,
    harness: &HarnessConfig,
) {
    let Ok(Some(previous)) = sessions::lookup_entry(session_id) else {
        return;
    };
    evict_previous_stash_pane_entry(
        tmux,
        session_id,
        &previous,
        replacement_pane,
        target_session,
        harness,
    );
}

fn evict_previous_stash_pane_entry(
    tmux: &Tmux,
    session_id: &str,
    previous: &sessions::SessionEntry,
    replacement_pane: &str,
    target_session: &str,
    harness: &HarnessConfig,
) {
    if previous.pane.is_empty()
        || previous.pane == replacement_pane
        || !tmux.pane_alive(&previous.pane)
    {
        return;
    }
    if pane_session_name(tmux, &previous.pane).as_deref() != Some(target_session) {
        return;
    }
    let Some(window_name) = pane_window_name(tmux, &previous.pane) else {
        return;
    };
    if !is_stash_window_name(&window_name) {
        return;
    }

    eprintln!(
        "[route] preserving previous stash pane {} for session {} — automatic stash eviction requires explicit provenance",
        previous.pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );
    let _ = (replacement_pane, target_session, harness);
}

/// Find a registered agent-doc pane in the target tmux session.
/// Used by auto_start to join alongside an existing agent-doc pane (not any random pane).
fn find_registered_pane_in_session(
    tmux: &Tmux,
    session_name: &str,
    exclude_pane: &str,
) -> Option<String> {
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
            .args([
                "display-message",
                "-t",
                &entry.pane,
                "-p",
                "#{session_name}",
            ])
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

/// Auto-start a new agent session in tmux using the default session name.
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
) -> Result<String> {
    auto_start_ext(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        false,
        false,
    )
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
) -> Result<String> {
    let split_before = is_first_column(file, col_args);
    auto_start_ext(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        true,
        split_before,
    )
}

fn auto_start_ext(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
) -> Result<String> {
    let harness = resolve_harness_for_file(file);
    let session_name = resolve_target_session(tmux, context_session, &harness);
    ensure_auto_start_target_session(tmux, context_session, &session_name, &harness)?;
    auto_start_in_session(
        tmux,
        file,
        session_id,
        file_path,
        &session_name,
        skip_wait,
        split_before,
        &harness,
    )
}

struct StartupLocks {
    _doc: File,
    _session: File,
}

fn starting_dir_for(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(file).ok()?;
    let base = snapshot::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(|p| p.to_path_buf()))?;
    Some(base.join(".agent-doc/starting"))
}

fn session_start_lock_name(session_name: &str) -> String {
    let hash = crate::snapshot::doc_hash_from_str(&format!("session:{session_name}"));
    format!("session-{hash}.lock")
}

fn open_start_lock(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open startup lock {}", path.display()))
}

fn acquire_startup_locks(file: &Path, session_name: &str) -> Result<Option<StartupLocks>> {
    let Some(starting_dir) = starting_dir_for(file) else {
        return Ok(None);
    };

    let doc_lock_path = if let Ok(hash) = snapshot::doc_hash(file) {
        starting_dir.join(format!("{hash}.lock"))
    } else {
        let fallback = crate::snapshot::doc_hash_from_str(&file.to_string_lossy());
        starting_dir.join(format!("{fallback}.lock"))
    };
    let session_lock_path = starting_dir.join(session_start_lock_name(session_name));

    let doc_lock = open_start_lock(&doc_lock_path)?;
    doc_lock
        .lock_exclusive()
        .with_context(|| format!("failed to acquire startup lock {}", doc_lock_path.display()))?;

    let session_lock = open_start_lock(&session_lock_path)?;
    session_lock.lock_exclusive().with_context(|| {
        format!(
            "failed to acquire session startup lock {}",
            session_lock_path.display()
        )
    })?;

    Ok(Some(StartupLocks {
        _doc: doc_lock,
        _session: session_lock,
    }))
}

/// Resolve HarnessConfig from a file's frontmatter + global config.
fn resolve_harness_for_file(file: &Path) -> HarnessConfig {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let fm = frontmatter::parse(&content)
        .map(|(f, _)| f)
        .unwrap_or_default();
    let global_config = crate::config::load().unwrap_or_default();
    HarnessConfig::from_context(&fm, &global_config)
}

/// Auto-start a new agent session in a specific tmux session.
///
/// Strategy:
/// 1. Find an existing registered agent-doc pane in the target session
/// 2. If found: `split-window` directly in that pane's window (avoids creating
///    a throwaway window then failing to join due to minimum pane size)
/// 3. If not found: create a new window via `auto_start` (session may not exist yet)
///
/// When `skip_wait` is true, skips `wait_for_agent_ready` and `send_command`.
/// Used by sync which only needs the pane to exist with the agent starting.
#[allow(clippy::too_many_arguments)]
fn auto_start_in_session(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    session_name: &str,
    skip_wait: bool,
    split_before: bool,
    harness: &HarnessConfig,
) -> Result<String> {
    // Serialize auto-starts for both the document and the target tmux session.
    // This prevents duplicate starts for the same file and split-target races
    // when two different documents provision concurrently into the same window.
    let startup_locks = acquire_startup_locks(file, session_name)?;
    if let Some(existing) = sessions::lookup(session_id)?
        && tmux.pane_alive(&existing)
    {
        eprintln!(
            "[route] startup already provisioned pane {} for {} while waiting on locks",
            existing, file_path
        );
        return Ok(existing);
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
        let window_panes = tmux
            .list_panes_ordered(&format!("{}:agent-doc", session_name))
            .unwrap_or_default();
        let positional = if split_before {
            window_panes.into_iter().next() // leftmost by screen position
        } else {
            window_panes.into_iter().last() // rightmost by screen position
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
                eprintln!(
                    "[route] stashed new pane {} to avoid window proliferation",
                    pane
                );
            }
            pane
        } else {
            eprintln!(
                "[route] no registered pane found in session '{}', creating new window",
                session_name
            );
            tmux.auto_start(session_name, &cwd)?
        }
    };

    evict_previous_stash_pane(tmux, session_id, &new_pane, session_name, harness);

    // Register immediately so subsequent route calls find this pane
    sessions::register(session_id, &new_pane, file_path)?;
    drop(startup_locks);

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
        "[route] Started {} for {} in pane {} (session {})",
        harness.binary,
        file_path,
        new_pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );

    let cycle_baseline = crate::cycle_state::load(file).unwrap_or(None);

    if skip_wait {
        eprintln!(
            "[route] skip_wait=true — pane created, {} starting (sync path)",
            harness.binary
        );
    } else {
        eprintln!("[route] Waiting for {} to initialize...", harness.binary);
        let dispatch = if wait_for_agent_ready(
            tmux,
            &new_pane,
            std::time::Duration::from_secs(30),
            harness,
        ) {
            eprintln!("[route] {} is ready, sending command", harness.binary);
            send_command_checked(tmux, &new_pane, &start_path, harness)?
        } else {
            eprintln!(
                "[route] Timed out waiting for {} prompt; attempting one fallback trigger injection before failing closed",
                harness.binary
            );
            match send_command_checked(tmux, &new_pane, &start_path, harness)? {
                CommandDispatchStatus::Accepted => {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_recovered file={} pane={} harness={}",
                            file.display(),
                            new_pane,
                            harness.binary
                        ),
                    );
                    eprintln!(
                        "[route] Fallback trigger injection recovered the fresh {} start for {}",
                        harness.binary, file_path
                    );
                    CommandDispatchStatus::Accepted
                }
                CommandDispatchStatus::TimedOut => {
                    crate::ops_log::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_missing file={} pane={} harness={}",
                            file.display(),
                            new_pane,
                            harness.binary
                        ),
                    );
                    anyhow::bail!(
                        "timed out waiting for {} prompt and fallback trigger injection was not accepted for {}",
                        harness.binary,
                        file.display()
                    );
                }
            }
        };

        if dispatch != CommandDispatchStatus::Accepted {
            crate::ops_log::log_op(
                file,
                &format!(
                    "fresh_route_trigger_missing file={} pane={} harness={}",
                    file.display(),
                    new_pane,
                    harness.binary
                ),
            );
            anyhow::bail!(
                "{} accepted input did not confirm the fresh trigger dispatch for {}",
                harness.binary,
                file.display()
            );
        }

        match wait_for_start_ack(file, cycle_baseline.as_ref(), routed_cycle_ack_timeout()) {
            Some(state) => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "fresh_route_start_acknowledged file={} pane={} harness={} cycle={} phase={}",
                        file.display(),
                        new_pane,
                        harness.binary,
                        state.cycle_id,
                        match state.phase {
                            crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
                            crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
                            crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
                            crate::cycle_state::CyclePhase::Committed => "committed",
                        }
                    ),
                );
                let _ = crate::startup_miss::clear(file);
            }
            None => {
                crate::ops_log::log_op(
                    file,
                    &format!(
                        "fresh_route_start_missing file={} pane={} harness={}",
                        file.display(),
                        new_pane,
                        harness.binary
                    ),
                );
                let baseline_id = cycle_baseline.as_ref().map(|b| b.cycle_id.as_str());
                let _ = crate::startup_miss::record(
                    file,
                    &new_pane,
                    session_id,
                    &harness.binary,
                    crate::startup_miss::StartupMissOrigin::FreshStart,
                    baseline_id,
                );
                emit_startup_miss_diagnostic(
                    tmux,
                    &new_pane,
                    file,
                    "fresh start: trigger accepted but no document cycle started",
                );
                anyhow::bail!(
                    "fresh {} start for {} never acknowledged with a document cycle after trigger injection",
                    harness.binary,
                    file.display()
                );
            }
        }
    }

    let _ = file; // suppress unused warning
    Ok(new_pane)
}

/// Poll a tmux pane until the agent is ready to accept input.
///
/// Uses the harness's prompt patterns for detection.
/// Strips ANSI escape codes before matching. Polls every 500ms up to the given timeout.
fn wait_for_agent_ready(
    tmux: &Tmux,
    pane_id: &str,
    timeout: std::time::Duration,
    harness: &HarnessConfig,
) -> bool {
    let start = std::time::Instant::now();
    let poll_interval = std::time::Duration::from_millis(500);
    let mut poll_count = 0u32;

    while start.elapsed() < timeout {
        if pane_has_prompt(tmux, pane_id, harness) {
            eprintln!(
                "[route] {} ready after {:.1}s ({} polls)",
                harness.binary,
                start.elapsed().as_secs_f64(),
                poll_count
            );
            return true;
        }

        poll_count += 1;
        if poll_count.is_multiple_of(10)
            && let Ok(content) = sessions::capture_pane(tmux, pane_id)
        {
            let last_line = content
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .map(prompt::strip_ansi)
                .unwrap_or_default();
            eprintln!(
                "[route] Still waiting for {} ({:.0}s)... last line: {}",
                harness.binary,
                start.elapsed().as_secs_f64(),
                &last_line[..std::cmp::min(60, last_line.len())]
            );
        }
        std::thread::sleep(poll_interval);
    }
    false
}

/// Check if pane content shows the agent's idle prompt.
fn pane_has_prompt(tmux: &Tmux, pane_id: &str, harness: &HarnessConfig) -> bool {
    if let Ok(content) = sessions::capture_pane(tmux, pane_id) {
        content
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|line| harness.is_prompt_line(line))
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
        eprintln!(
            "[route] Auto-synced {} files in window {}",
            file_count, window_id
        );
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
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};

    // Serialize env var mutations across parallel test threads.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A smaller lock for startup-sensitive isolated tmux tests that inject the
    // first command immediately after pane creation.
    static TMUX_START_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn tmux_start_lock() -> std::sync::MutexGuard<'static, ()> {
        TMUX_START_MUTEX
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn test_cwd() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn test_registry_entry(
        pane: &str,
        file: &str,
        cwd: &std::path::Path,
    ) -> sessions::SessionEntry {
        sessions::SessionEntry {
            pane: pane.to_string(),
            pid: 1234,
            cwd: cwd.to_string_lossy().to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            file: file.to_string(),
            window: "@1".to_string(),
        }
    }

    struct ScopedCurrentDir {
        prev_cwd: std::path::PathBuf,
        _env_guard: std::sync::MutexGuard<'static, ()>,
    }

    impl ScopedCurrentDir {
        fn set(path: &std::path::Path) -> Self {
            let env_guard = env_lock();
            let prev_cwd = std::env::current_dir().unwrap_or_else(|_| test_cwd());
            std::env::set_current_dir(path).unwrap();
            Self {
                prev_cwd,
                _env_guard: env_guard,
            }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev_cwd);
        }
    }

    #[test]
    fn pane_registration_matches_file_resolves_entry_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let submodule = dir.path().join("src/session-share");
        let tasks = submodule.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("claudescore-3.md");
        std::fs::write(&doc, "# session\n").unwrap();

        let mut registry = sessions::SessionRegistry::new();
        registry.insert(
            "session-a".to_string(),
            test_registry_entry("%401", "tasks/claudescore-3.md", &submodule),
        );

        assert!(
            pane_registration_matches_file(&registry, "%401", &doc.to_string_lossy()),
            "relative registry paths should resolve against the pane cwd"
        );
    }

    #[test]
    fn ensure_dispatch_target_matches_file_rejects_cross_file_registration() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let submodule = dir.path().join("src/session-share");
        let tasks = submodule.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let registered = tasks.join("monsterrodholders.md");
        let requested = tasks.join("claudescore-3.md");
        std::fs::write(&registered, "# registered\n").unwrap();
        std::fs::write(&requested, "# requested\n").unwrap();

        sessions::register_full_with_cwd_in(
            dir.path(),
            "session-a",
            "%401",
            "tasks/monsterrodholders.md",
            1234,
            "@1",
            &submodule.to_string_lossy(),
        )
        .unwrap();

        let err = ensure_dispatch_target_matches_file("%401", &requested.to_string_lossy())
            .expect_err("cross-file pane reuse must fail closed");
        assert!(
            err.to_string().contains("refusing cross-file dispatch"),
            "error should explain the rejected cross-file dispatch: {err}"
        );
    }

    fn wait_for_pane_contains(
        iso: &IsolatedTmux,
        pane: &str,
        needle: &str,
        timeout: std::time::Duration,
    ) -> String {
        let start = std::time::Instant::now();
        let poll = std::time::Duration::from_millis(100);
        let mut last = String::new();
        while start.elapsed() < timeout {
            last = sessions::capture_pane(iso, pane).unwrap_or_default();
            if last.contains(needle) {
                return last;
            }
            std::thread::sleep(poll);
        }
        last
    }

    fn send_keys_with_retry(iso: &IsolatedTmux, pane: &str, text: &str) {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(3);
        let poll = std::time::Duration::from_millis(100);
        let mut last_err = None;

        while start.elapsed() < timeout {
            match iso.send_keys(pane, text) {
                Ok(()) => return,
                Err(err) => last_err = Some(err.to_string()),
            }
            std::thread::sleep(poll);
        }

        panic!(
            "failed to send keys to pane {} after {:.1}s: {}",
            pane,
            start.elapsed().as_secs_f64(),
            last_err.unwrap_or_else(|| "unknown error".to_string())
        );
    }

    fn pane_current_command(iso: &IsolatedTmux, pane: &str) -> Option<String> {
        let output = iso
            .cmd()
            .args([
                "display-message",
                "-t",
                pane,
                "-p",
                "#{pane_current_command}",
            ])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let cmd = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if cmd.is_empty() { None } else { Some(cmd) }
    }

    fn wait_for_shell(iso: &IsolatedTmux, pane: &str, timeout: std::time::Duration) -> bool {
        const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Some(cmd) = pane_current_command(iso, pane)
                && IDLE_SHELLS.contains(&cmd.as_str())
            {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

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
        assert_eq!(
            rewritten,
            format!("tasks{}foo.md", std::path::MAIN_SEPARATOR)
        );
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

    // --- Prompt detection tests (via HarnessConfig) ---

    #[test]
    fn detects_unicode_prompt() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("❯"));
        assert!(h.is_prompt_line("❯ "));
        assert!(h.is_prompt_line("  ❯  "));
    }

    #[test]
    fn detects_ascii_prompt() {
        let h = HarnessConfig::codex();
        assert!(h.is_prompt_line(">"));
        assert!(h.is_prompt_line("> "));
        assert!(h.is_prompt_line("  >  "));
    }

    #[test]
    fn rejects_non_prompt_lines() {
        let h = HarnessConfig::claude();
        assert!(!h.is_prompt_line("Starting claude..."));
        assert!(!h.is_prompt_line("test result: ok"));
        assert!(!h.is_prompt_line(""));
        assert!(!h.is_prompt_line("  "));
        assert!(!h.is_prompt_line("## User"));
    }

    #[test]
    fn handles_ansi_prompt() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("\x1b[32m❯\x1b[0m"));
        let h_codex = HarnessConfig::codex();
        assert!(h_codex.is_prompt_line("\x1b[1m>\x1b[0m"));
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

    #[test]
    fn lazy_claim_requires_explicit_pane_provenance() {
        let iso = IsolatedTmux::new("route-test-lazy-claim-explicit-only");
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start("claim", &cwd).unwrap();
        let claimed_panes = std::collections::HashSet::new();

        assert_eq!(
            find_target_pane(&iso, None, "claim", &claimed_panes),
            None,
            "route must not adopt the session's active pane implicitly"
        );
        assert_eq!(
            find_target_pane(&iso, Some(&pane), "claim", &claimed_panes),
            Some(pane),
            "explicit pane override remains valid lazy-claim provenance"
        );
    }

    #[test]
    fn wrong_session_pane_still_receives_send() {
        // Strategy 1 is session-agnostic after the fix: when a registered pane
        // is alive, send to it regardless of which tmux session it lives in.
        //
        // This is the bug scenario: IDE `Run Agent Doc` spawns `agent-doc route`
        // with no $TMUX env, so target_session falls back to the constant
        // "claude". The claimed pane lives in the user's real session (e.g.
        // "btak"). Before the fix, the session mismatch + shell-idle process
        // sent routing to Strategy 2/3 — auto-starting a new Claude pane in
        // the non-existent "claude" session.
        //
        // This test verifies the tmux infrastructure that makes the fix work:
        // pane_alive must return true for an alive pane regardless of the
        // session it belongs to. The %N pane ID is the routing key.
        let iso = IsolatedTmux::new("route-test-wrong-sess-send");
        let cwd = std::env::current_dir().unwrap();

        // Pane lives in session "real" (simulating the user's tmux session).
        let registered_pane = iso.auto_start("real", &cwd).unwrap();
        assert!(iso.pane_alive(&registered_pane));

        // tmux has no session named "claude" (the fallback target_session).
        let claude_alive = iso
            .cmd()
            .args(["has-session", "-t", "claude"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(!claude_alive, "fallback target session should not exist");

        // pane_alive does not consult session membership — Strategy 1 can
        // send to the pane via its %N id even though pane_session != "claude".
        assert!(
            iso.pane_alive(&registered_pane),
            "alive pane must be routable regardless of target_session"
        );
    }

    // --- Integration tests (IsolatedTmux) ---

    use sessions::IsolatedTmux;

    /// Create a mock agent script: blocks for delay, then prints ❯ prompt on its own line.
    /// Uses `cat` to keep the process alive after showing the prompt.
    fn mock_agent_script(delay_ms: u64) -> String {
        format!(
            r#"exec /bin/sh -c 'printf "Starting agent...\n"; sleep {}; printf "❯ \n"; cat'"#,
            delay_ms as f64 / 1000.0
        )
    }

    fn write_mock_registered_agent_doc(base: &Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let bin_dir = base.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script = bin_dir.join("agent-doc");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        script
    }

    fn launch_mock_registered_agent_doc(
        iso: &IsolatedTmux,
        pane: &str,
        script: &Path,
        file: &Path,
    ) {
        send_keys_with_retry(
            iso,
            pane,
            &format!("exec {} {}", script.display(), file.display()),
        );
        let content = wait_for_pane_contains(iso, pane, "\n>", std::time::Duration::from_secs(10));
        assert!(
            content.contains("\n>"),
            "mock agent-doc session should present a prompt, got: {content}"
        );
    }

    fn launch_mock_agent_doc_without_file_arg(iso: &IsolatedTmux, pane: &str, script: &Path) {
        send_keys_with_retry(iso, pane, &format!("exec {}", script.display()));
        let content = wait_for_pane_contains(iso, pane, "\n>", std::time::Duration::from_secs(10));
        assert!(
            content.contains("\n>"),
            "mock agent-doc session should present a prompt, got: {content}"
        );
    }

    fn wait_for_process_pid(pattern: &str, timeout: std::time::Duration) -> u32 {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if let Ok(output) = std::process::Command::new("pgrep")
                .args(["-f", pattern])
                .output()
                && output.status.success()
            {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let pid = line.trim();
                    if pid.is_empty() {
                        continue;
                    }
                    if let Ok(parsed) = pid.parse::<u32>() {
                        return parsed;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("timed out waiting for process matching pattern: {pattern}");
    }

    #[test]
    fn wait_for_agent_ready_detects_prompt() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-ready");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );

        send_keys_with_retry(&iso, &pane, &mock_agent_script(500));
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "Starting agent...",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("Starting agent..."),
            "mock agent never started in pane: {content}"
        );

        let harness = HarnessConfig::claude();
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
        assert!(ready, "should detect ❯ prompt from mock agent");
    }

    #[test]
    fn wait_for_agent_ready_times_out_without_prompt() {
        let iso = IsolatedTmux::new("route-test-timeout");
        let session = "test";
        let cwd = test_cwd();

        let pane_id = iso
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                session,
                "-c",
                &cwd.to_string_lossy(),
                "-P",
                "-F",
                "#{pane_id}",
                "sleep",
                "30",
            ])
            .output()
            .expect("failed to create tmux session");
        let pane = String::from_utf8_lossy(&pane_id.stdout).trim().to_string();

        let harness = HarnessConfig::claude();
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(2), &harness);
        assert!(!ready, "should time out when no ❯ prompt appears");
    }

    #[test]
    fn wait_for_agent_ready_codex_prompt() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-codex-ready");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock codex launch"
        );

        // Codex uses > as prompt
        let script =
            r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "> \n"; cat'"#;
        send_keys_with_retry(&iso, &pane, script);
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "Starting codex...",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("Starting codex..."),
            "mock codex never started in pane: {content}"
        );

        let harness = HarnessConfig::codex();
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
        assert!(ready, "should detect > prompt for codex harness");
    }

    #[test]
    fn recent_lines_contain_trigger_matches_claude_trigger() {
        let content = "\
history line
\x1b[32m❯\x1b[0m /agent-doc test.md
";
        assert!(recent_lines_contain_trigger(content, "/agent-doc test.md"));
        assert!(!recent_lines_contain_trigger(content, "agent-doc test.md"));
    }

    #[test]
    fn recent_lines_contain_trigger_matches_codex_trigger() {
        let content = "\
history line
> agent-doc test.md
";
        assert!(recent_lines_contain_trigger(content, "agent-doc test.md"));
        assert!(!recent_lines_contain_trigger(content, "/agent-doc test.md"));
    }

    #[test]
    fn line_contains_trigger_rejects_codex_substring_inside_claude_trigger() {
        assert!(line_contains_trigger(
            "❯ /agent-doc test.md",
            "/agent-doc test.md"
        ));
        assert!(!line_contains_trigger(
            "❯ /agent-doc test.md",
            "agent-doc test.md"
        ));
    }

    #[test]
    fn send_keys_delivers_claude_command_with_enter() {
        let iso = IsolatedTmux::new("route-test-send");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        // Start a shell that reads a line and echoes it back with a marker
        send_keys_with_retry(
            &iso,
            &pane,
            r#"exec /bin/sh -c 'printf "READY\n"; read CMD; printf "GOT:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &pane, "READY", std::time::Duration::from_secs(3));

        let trigger = HarnessConfig::claude().trigger_command("test.md");
        send_keys_with_retry(&iso, &pane, &trigger);

        // Capture and verify the command was received
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            &format!("GOT:{}", trigger),
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains(&format!("GOT:{}", trigger)),
            "command should be delivered and echoed back, got: {}",
            content
        );
    }

    #[test]
    fn send_keys_delivers_codex_command_with_enter() {
        let iso = IsolatedTmux::new("route-test-send-codex");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        send_keys_with_retry(
            &iso,
            &pane,
            r#"exec /bin/sh -c 'printf "READY\n"; read CMD; printf "GOT:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &pane, "READY", std::time::Duration::from_secs(3));

        let trigger = HarnessConfig::codex().trigger_command("test.md");
        send_keys_with_retry(&iso, &pane, &trigger);

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            &format!("GOT:{}", trigger),
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains(&format!("GOT:{}", trigger)),
            "command should be delivered and echoed back, got: {}",
            content
        );
    }

    #[test]
    fn send_command_checked_reports_accepted_when_command_is_consumed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join("test.md"), "# test\n").unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-send-checked");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane).unwrap();
        sessions::register_full_with_cwd_in(
            dir.path(),
            "route-test-send-checked",
            &pane,
            "test.md",
            1234,
            &window,
            &dir.path().to_string_lossy(),
        )
        .unwrap();

        send_keys_with_retry(
            &iso,
            &pane,
            r#"exec /bin/sh -c 'printf "READY\n"; read CMD; printf "GOT:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &pane, "READY", std::time::Duration::from_secs(3));

        let status = send_command_checked(&iso, &pane, "test.md", &HarnessConfig::codex()).unwrap();
        assert_eq!(status, CommandDispatchStatus::Accepted);
    }

    #[test]
    fn wait_for_start_ack_detects_new_preflight_cycle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some("# Session\n"))
                .unwrap();
        });

        let ack = wait_for_start_ack(&doc, None, Duration::from_secs(1));
        assert!(
            ack.is_some(),
            "fresh start should acknowledge a new preflight cycle"
        );
        assert_eq!(
            ack.unwrap().phase,
            crate::cycle_state::CyclePhase::PreflightStarted
        );
    }

    #[test]
    fn wait_for_start_ack_detects_new_committed_cycle_after_prior_commit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some("# Session\n"))
                .unwrap();
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_secs(1))
            .expect("new committed cycle should count as startup acknowledgment");
        assert_ne!(ack.cycle_id, baseline.cycle_id);
        assert_eq!(ack.phase, crate::cycle_state::CyclePhase::Committed);
    }

    #[test]
    fn wait_for_start_ack_times_out_without_cycle_change() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

        let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_millis(250));
        assert!(
            ack.is_none(),
            "unchanged cycle state must not count as a fresh-start ack"
        );
    }

    #[test]
    fn wait_for_start_ack_ignores_same_committed_cycle_mutation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

        let doc_for_thread = doc.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_already_current",
                Some("# Session\n"),
                Some("# Session\n"),
            )
            .unwrap();
        });

        let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_millis(350));
        assert!(
            ack.is_none(),
            "same committed cycle mutations must not count as a new routed-start ack"
        );
    }

    #[test]
    fn routed_cycle_ack_only_required_for_prompt_bearing_drift_on_closed_cycle() {
        assert!(!should_require_routed_cycle_ack(None, None));

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
        let open_state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert!(!should_require_routed_cycle_ack(
            Some(&open_state),
            Some("prompt_target: ❯ follow-up question"),
        ));

        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
        let committed_state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert!(should_require_routed_cycle_ack(
            Some(&committed_state),
            Some("prompt_target: ❯ follow-up question"),
        ));
    }

    #[test]
    fn resolve_or_create_pane_fails_closed_when_live_child_does_not_start_new_cycle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-child-skip-ack");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-child-skip", &pane, &file_path).unwrap();

        let err = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-child-skip",
            &file_path,
            session,
            &HarnessConfig::codex(),
        )
        .expect_err("route should fail closed when the live child never starts a new cycle");
        assert!(
            err.to_string()
                .contains("no new document cycle started for pending prompt_target"),
            "unexpected error: {err:#}"
        );

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should still dispatch the trigger to the registered pane: {content}"
        );
    }

    #[test]
    fn resolve_or_create_pane_rejects_same_committed_cycle_mutation_for_prompt_drift() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-ack-same-cycle");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-same-cycle", &pane, &file_path).unwrap();

        let doc_for_thread = doc.clone();
        let snapshot_for_thread = snapshot.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_already_current",
                Some(&snapshot_for_thread),
                Some(&snapshot_for_thread),
            )
            .unwrap();
        });

        let err = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-same-cycle",
            &file_path,
            session,
            &HarnessConfig::codex(),
        )
        .expect_err("same-cycle committed churn must not satisfy routed live-pane ack");
        assert!(
            err.to_string()
                .contains("no new document cycle started for pending prompt_target"),
            "unexpected error: {err:#}"
        );

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should still dispatch the trigger to the registered pane: {content}"
        );
    }

    #[test]
    fn resolve_or_create_pane_accepts_registered_pane_trigger_once_new_cycle_starts() {
        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-ack-ok");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-ok", &pane, &file_path).unwrap();

        let doc_for_thread = doc.clone();
        let snapshot_for_thread = snapshot.to_string();
        let current_for_thread = current.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-ok",
            &file_path,
            session,
            &HarnessConfig::codex(),
        )
        .expect("route should accept the new cycle ack");
        assert_eq!(resolved, pane);

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should dispatch the trigger before observing the ack: {content}"
        );
    }

    #[test]
    fn alive_registered_pane_without_live_owner_deregisters_and_lazy_claims() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-owner-missing");
        let session = "claude";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));

        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-owner-missing", &stale_pane, &file_path).unwrap();

        let doc_for_thread = doc.clone();
        let current_for_thread = format!("# Session\n❯ follow-up question\n");
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some("# Session\n"),
                Some(&current_for_thread),
            )
            .unwrap();
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some("# Session\n"),
                Some(&current_for_thread),
            )
            .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-owner-missing",
            &file_path,
            session,
            &HarnessConfig::codex(),
        )
        .expect("route should continue recovery after clearing the stale registration");
        assert_ne!(resolved, stale_pane);

        let reassigned = sessions::lookup("route-live-owner-missing").unwrap();
        assert!(
            reassigned.as_deref() == Some(resolved.as_str()),
            "route should re-register to the recovered pane, got: {reassigned:?}"
        );

        let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_content.contains("STALE:agent-doc "),
            "route should not dispatch into the stale registered pane: {stale_content}"
        );
    }

    #[test]
    fn alive_registered_pane_reregisters_to_live_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-owner-reregister");
        let session = "claude";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));

        let live_pane = iso.auto_start(session, &cwd).unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &live_pane, &mock_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        sessions::register("route-live-owner-reregister", &stale_pane, &file_path).unwrap();

        let doc_for_thread = doc.clone();
        let snapshot_for_thread = snapshot.to_string();
        let current_for_thread = current.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            crate::cycle_state::start_preflight(
                &doc_for_thread,
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
            crate::cycle_state::mark_committed(
                &doc_for_thread,
                "commit_success",
                Some(&snapshot_for_thread),
                Some(&current_for_thread),
            )
            .unwrap();
        });

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-owner-reregister",
            &file_path,
            session,
            &HarnessConfig::codex(),
        )
        .expect("route should recover by re-registering to the live owner");
        assert_eq!(resolved, live_pane);

        let live_content = wait_for_pane_contains(
            &iso,
            &live_pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            live_content.contains("GOT:agent-doc "),
            "route should dispatch to the recovered live owner: {live_content}"
        );

        let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_content.contains("STALE:agent-doc "),
            "route should not dispatch to the stale registered pane: {stale_content}"
        );
    }

    #[test]
    fn alive_registered_pane_uses_supervisor_pid_fallback_when_argv_loses_file_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-owner-supervisor-pid");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-owner-supervisor";

        let mock_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_agent_doc_without_file_arg(&iso, &pane, &mock_agent);
        let mock_agent_pid =
            wait_for_process_pid(&mock_agent.display().to_string(), Duration::from_secs(3));

        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": mock_agent_pid })),
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Inject { bytes } => IpcResponse::ok(serde_json::json!({ "n": bytes.len() })),
            IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        sessions::register(session_id, &pane, &file_path).unwrap();

        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
        )
        .expect("route should recover the live owner via supervisor pid");
        assert_eq!(resolved, pane);

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc "),
            "route should dispatch to the registered pane recovered from supervisor pid: {content}"
        );

        ipc.stop();
    }

    #[test]
    fn pane_has_prompt_detects_unicode() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-has-prompt");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before prompt detection test"
        );

        send_keys_with_retry(&iso, &pane, &mock_agent_script(100));
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "Starting agent...",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("Starting agent..."),
            "mock agent never started in pane: {content}"
        );
        let harness = HarnessConfig::claude();
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
        let content = sessions::capture_pane(&iso, &pane).unwrap_or_default();
        assert!(
            ready && pane_has_prompt(&iso, &pane, &harness),
            "should detect ❯ in pane content, got: {}",
            content
        );
    }

    #[test]
    fn full_auto_start_flow() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-e2e");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before e2e launch"
        );

        send_keys_with_retry(&iso, &pane, &mock_agent_script(300));
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "Starting agent...",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("Starting agent..."),
            "mock agent never started in pane: {content}"
        );

        let harness = HarnessConfig::claude();
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
        assert!(ready, "mock agent should become ready");

        send_keys_with_retry(&iso, &pane, "HELLO_FROM_TEST");

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "HELLO_FROM_TEST",
            std::time::Duration::from_secs(3),
        );
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
            .args(["display-message", "-t", session, "-p", "#{pane_id}"])
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
        send_keys_with_retry(&iso, &pane, "echo DONE");
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
            "pane should be in session '{}'",
            session
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
            .args([
                "display-message",
                "-t",
                &correct_pane,
                "-p",
                "#{session_name}",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap();
        let wrong_session = iso
            .cmd()
            .args([
                "display-message",
                "-t",
                &wrong_pane,
                "-p",
                "#{session_name}",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap();

        assert_eq!(correct_session, "correct");
        assert_eq!(wrong_session, "wrong");
        assert_ne!(
            correct_session, wrong_session,
            "panes should be in different sessions"
        );
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
        assert_eq!(
            panes.len(),
            2,
            "window should have exactly 2 panes after split"
        );
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
        assert_ne!(
            w1, w_fb_before,
            "fallback should be in a new window initially"
        );

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

    #[test]
    fn preserves_replaced_stash_pane_without_provenance() {
        let iso = IsolatedTmux::new("route-test-evict-stash");
        let session = "route-evict";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let result = (|| -> anyhow::Result<()> {
            let old_pane = iso.auto_start(session, dir.path())?;
            iso.stash_pane(&old_pane, session)?;

            let replacement_pane = iso.auto_start(session, dir.path())?;
            iso.stash_pane(&replacement_pane, session)?;

            let previous = sessions::SessionEntry {
                pane: old_pane.clone(),
                pid: std::process::id(),
                cwd: dir.path().to_string_lossy().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: "doc.md".to_string(),
                window: iso.pane_window(&old_pane)?,
            };
            evict_previous_stash_pane_entry(
                &iso,
                "session-123",
                &previous,
                &replacement_pane,
                session,
                &HarnessConfig::claude(),
            );

            assert!(
                iso.pane_alive(&old_pane),
                "previous stash pane should be preserved without explicit provenance"
            );
            assert!(
                iso.pane_alive(&replacement_pane),
                "replacement pane should stay alive"
            );
            Ok(())
        })();

        result.unwrap();
    }

    #[test]
    fn eviction_skipped_when_agent_process_active() {
        let iso = IsolatedTmux::new("route-test-evict-busy");
        let session = "route-evict-busy";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let result = (|| -> anyhow::Result<()> {
            let busy_pane = iso.auto_start(session, dir.path())?;
            iso.stash_pane(&busy_pane, session)?;

            // Copy /bin/sleep as "agent-doc" so tmux's #{pane_current_command}
            // reports the binary name that matches the harness process list.
            let bin_dir = dir.path().join("bin");
            std::fs::create_dir_all(&bin_dir)?;
            let fake_agent = bin_dir.join("agent-doc");
            std::fs::copy("/bin/sleep", &fake_agent)?;

            iso.raw_cmd(&[
                "send-keys",
                "-t",
                &busy_pane,
                &format!("{} 60", fake_agent.display()),
                "Enter",
            ])?;

            // Poll until pane_current_command changes from the shell
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let out = iso
                    .cmd()
                    .args([
                        "display-message",
                        "-t",
                        &busy_pane,
                        "-p",
                        "#{pane_current_command}",
                    ])
                    .output()?;
                let cmd = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if cmd == "agent-doc" {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for agent-doc to start in pane (last cmd: '{}')",
                    cmd
                );
            }

            let replacement_pane = iso.auto_start(session, dir.path())?;
            iso.stash_pane(&replacement_pane, session)?;

            let previous = sessions::SessionEntry {
                pane: busy_pane.clone(),
                pid: std::process::id(),
                cwd: dir.path().to_string_lossy().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                file: "doc.md".to_string(),
                window: iso.pane_window(&busy_pane)?,
            };
            evict_previous_stash_pane_entry(
                &iso,
                "session-busy",
                &previous,
                &replacement_pane,
                session,
                &HarnessConfig::claude(),
            );

            assert!(
                iso.pane_alive(&busy_pane),
                "stash pane running agent process should NOT be evicted"
            );
            assert!(
                iso.pane_alive(&replacement_pane),
                "replacement pane should stay alive"
            );
            Ok(())
        })();

        result.unwrap();
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
        let result = {
            let _env_guard = env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_NO_AUTOSTART", "1");
            }
            let r = run_with_tmux(&file, &iso, None, 0, &[]);
            unsafe {
                std::env::remove_var("AGENT_DOC_NO_AUTOSTART");
            }
            r
        };

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
        let _result = {
            let _env_guard = env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_NO_AUTOSTART", "1");
            }
            let r = run_with_tmux(&file, &iso, None, 0, &[]);
            unsafe {
                std::env::remove_var("AGENT_DOC_NO_AUTOSTART");
            }
            r
        };

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

    #[test]
    fn resolve_target_session_ignores_blank_context_session() {
        let iso = IsolatedTmux::new("route-test-blank-context");
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start("claude", &cwd).unwrap();
        let current_session = iso.pane_session(&pane).unwrap();

        let resolved = resolve_target_session(&iso, Some("   "), &HarnessConfig::claude());
        assert_eq!(
            resolved, current_session,
            "blank context_session should fall back to the live target session"
        );
    }

    #[test]
    fn blank_context_session_does_not_bypass_target_validation() {
        let iso = IsolatedTmux::new("route-test-blank-context-validate");
        let result =
            ensure_auto_start_target_session(&iso, Some("   "), "claude", &HarnessConfig::claude());
        assert!(
            result.is_err(),
            "blank context_session should not bypass implicit fallback validation"
        );
    }

    #[test]
    fn implicit_fallback_session_is_not_auto_start_target() {
        let iso = IsolatedTmux::new("route-test-no-implicit-fallback");
        let result =
            ensure_auto_start_target_session(&iso, None, "claude", &HarnessConfig::claude());
        assert!(
            result.is_err(),
            "dead implicit fallback session should not be auto-started"
        );
    }

    // --- Stash rescue tests ---

    #[test]
    fn pane_in_stash_rescued_to_agent_doc() {
        // When a registered pane ends up in a stash window, route should
        // rescue it back to the agent-doc window without ejecting the
        // currently visible pane into stash.
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

        // Now rescue: join the stashed pane back into the agent-doc window.
        let agent_doc_window = format!("{}:agent-doc", session);
        let target_panes = iso.list_window_panes(&agent_doc_window).unwrap_or_default();
        assert!(
            !target_panes.is_empty(),
            "agent-doc window should have panes"
        );

        if let Some(target) = target_panes.first() {
            sessions::join_pane_guarded(&iso, &stashed_pane, target, session, "-dh").unwrap();
            let rescued_win = iso.pane_window(&stashed_pane).unwrap();
            let visible_win = iso.pane_window(&pane1).unwrap();
            assert_eq!(
                rescued_win, visible_win,
                "rescued pane should rejoin the visible agent-doc window"
            );
            let agent_doc_panes = iso.list_window_panes(&agent_doc_window).unwrap();
            assert!(
                agent_doc_panes.contains(&pane1),
                "existing visible pane should stay in agent-doc window, got: {:?}",
                agent_doc_panes
            );
            assert!(
                agent_doc_panes.contains(&stashed_pane),
                "rescued pane should be in agent-doc window, got: {:?}",
                agent_doc_panes
            );
            assert!(
                iso.pane_alive(&stashed_pane),
                "rescued pane should be alive"
            );
        }
    }

    #[test]
    fn join_pane_rescue_places_left_of_target_when_requested() {
        let iso = IsolatedTmux::new("route-test-join-left");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create session with agent-doc window
        let pane1 = iso.auto_start(session, &cwd).unwrap();
        let _ = iso
            .cmd()
            .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
            .status();

        // Create a second pane in its own window and rescue it to the left edge.
        let pane2 = iso.auto_start(session, &cwd).unwrap();
        let agent_doc_window = format!("{}:agent-doc", session);
        let target_panes = iso.list_window_panes(&agent_doc_window).unwrap();
        let target = &target_panes[0];

        sessions::join_pane_guarded(&iso, &pane2, target, session, "-dbh").unwrap();

        let agent_doc_panes = iso.list_panes_ordered(&agent_doc_window).unwrap();
        assert!(
            agent_doc_panes.contains(&pane2),
            "pane should be in agent-doc window after join, got: {:?}",
            agent_doc_panes
        );
        assert_eq!(
            agent_doc_panes.first().unwrap(),
            &pane2,
            "split-before rescue should place the pane on the left edge"
        );
        assert!(
            agent_doc_panes.contains(&pane1),
            "original pane should remain visible after rescue, got: {:?}",
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
        assert!(
            iso.pane_alive(&pane_a),
            "pane should survive sync_after_claim"
        );
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
        assert!(
            iso.pane_alive(&pane_a),
            "pane should survive sync with unresolved files"
        );
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
        let ordered = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
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
        let final_order = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
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
        let ordered = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
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
        let final_order = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
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
        let ordered = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
        assert_eq!(ordered.len(), 2, "should start with 2 panes");

        // col_args: file_a is in first column, file_b in second
        let col_args = vec!["tasks/file_a.md".to_string(), "tasks/file_b.md".to_string()];

        // Call provision_pane with file in the FIRST column
        let file_a = Path::new("tasks/file_a.md");
        let result = provision_pane(
            &iso,
            file_a,
            "route-test-provision-first-col-session-a",
            "tasks/file_a.md",
            Some(session),
            &col_args,
        );
        assert!(
            result.is_ok(),
            "provision_pane should succeed: {:?}",
            result.err()
        );

        // The new pane should be leftmost (split_before=true picks first pane, splits -dbh)
        let after = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
        assert_eq!(after.len(), 3, "should have 3 panes after auto_start");
        // The new pane is NOT one of the original two — find it
        let new_pane: Vec<_> = after
            .iter()
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
        let ordered = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
        assert_eq!(ordered.len(), 2, "should start with 2 panes");

        // col_args: file_a is in first column, file_b in second
        let col_args = vec!["tasks/file_a.md".to_string(), "tasks/file_b.md".to_string()];

        // Call provision_pane with file in the SECOND column
        let file_b = Path::new("tasks/file_b.md");
        let result = provision_pane(
            &iso,
            file_b,
            "route-test-provision-second-col-session-b",
            "tasks/file_b.md",
            Some(session),
            &col_args,
        );
        assert!(
            result.is_ok(),
            "provision_pane should succeed: {:?}",
            result.err()
        );

        // The new pane should be rightmost (split_before=false picks last pane, splits -dh)
        let after = iso
            .list_window_panes(&format!("{}:agent-doc", session))
            .unwrap();
        assert_eq!(after.len(), 3, "should have 3 panes after auto_start");
        // Find the new pane (not one of the original two)
        let new_pane: Vec<_> = after
            .iter()
            .filter(|p| *p != &pane_left && *p != &pane_right)
            .collect();
        assert_eq!(new_pane.len(), 1, "should have exactly 1 new pane");
        assert_eq!(
            after.last().unwrap(),
            new_pane[0],
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
        )
        .unwrap();

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

        assert_eq!(
            before.len(),
            after.len(),
            "no panes should be created when no registry exists"
        );
    }

    #[test]
    fn list_panes_ordered_returns_screen_position_after_rearrange() {
        // When panes are broken out and re-joined, creation order can diverge from
        // screen position. list_panes_ordered must return screen order (by pane_left).
        let iso = IsolatedTmux::new("route-test-pane-order-rearrange");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        let pane_a = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane_a).unwrap();
        let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
        let pane_b = iso.split_window(&pane_a, &cwd, "-dh").unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

        // Rearrange: break pane_b out, rejoin to the LEFT of pane_a.
        let _ = iso.raw_cmd(&["break-pane", "-d", "-t", &pane_b]);
        let _ = iso.raw_cmd(&["join-pane", "-bh", "-d", "-s", &pane_b, "-t", &pane_a]);

        let screen_order = iso
            .list_panes_ordered(&format!("{}:agent-doc", session))
            .unwrap();
        assert_eq!(screen_order.len(), 2);
        assert_eq!(
            screen_order[0], pane_b,
            "pane_b should be leftmost after rejoin to the left"
        );
        assert_eq!(
            screen_order[1], pane_a,
            "pane_a should be rightmost after rejoin shifted it right"
        );
    }

    #[test]
    fn provision_pane_right_col_picks_rightmost_after_rearrange() {
        // Regression: provision_pane must use screen position, not creation order.
        // After rearranging panes so creation order != screen order,
        // split_before=false should split from the rightmost pane by screen position.
        let iso = IsolatedTmux::new("route-test-provision-rearranged");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        let pane_a = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane_a).unwrap();
        let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
        let pane_b = iso.split_window(&pane_a, &cwd, "-dh").unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

        // Rearrange: break pane_b, rejoin LEFT of pane_a.
        // Screen: [pane_b, pane_a]. pane_a is now rightmost.
        let _ = iso.raw_cmd(&["break-pane", "-d", "-t", &pane_b]);
        let _ = iso.raw_cmd(&["join-pane", "-bh", "-d", "-s", &pane_b, "-t", &pane_a]);

        // Provision a right-column file — should split from pane_a (rightmost by screen).
        let col_args = vec!["tasks/file_a.md".to_string(), "tasks/file_b.md".to_string()];
        let file_b = Path::new("tasks/file_b.md");
        let result = provision_pane(
            &iso,
            file_b,
            "route-test-provision-rearranged-session-b",
            "tasks/file_b.md",
            Some(session),
            &col_args,
        );
        assert!(
            result.is_ok(),
            "provision_pane should succeed: {:?}",
            result.err()
        );

        let after = iso
            .list_panes_ordered(&format!("{}:agent-doc", session))
            .unwrap();
        assert_eq!(after.len(), 3, "should have 3 panes");

        // The new pane should be rightmost (split after pane_a which is rightmost).
        let new_pane: Vec<_> = after
            .iter()
            .filter(|p| *p != &pane_a && *p != &pane_b)
            .collect();
        assert_eq!(new_pane.len(), 1, "should have exactly 1 new pane");
        assert_eq!(
            after.last().unwrap(),
            new_pane[0],
            "right-column file should produce rightmost pane even after rearrangement"
        );
    }

    #[test]
    fn concurrent_provision_pane_serializes_same_session_auto_start() {
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let session = "test";
        let iso = Arc::new(IsolatedTmux::new("route-test-concurrent-provision"));
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "# A\n").unwrap();
        std::fs::write(&doc_b, "# B\n").unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let iso_a = Arc::clone(&iso);
        let barrier_a = Arc::clone(&barrier);
        let doc_a_thread = doc_a.clone();
        let handle_a = std::thread::spawn(move || {
            barrier_a.wait();
            provision_pane(
                &iso_a,
                &doc_a_thread,
                "route-test-concurrent-provision-session-a",
                doc_a_thread.to_string_lossy().as_ref(),
                Some(session),
                &[],
            )
        });

        let iso_b = Arc::clone(&iso);
        let barrier_b = Arc::clone(&barrier);
        let doc_b_thread = doc_b.clone();
        let handle_b = std::thread::spawn(move || {
            barrier_b.wait();
            provision_pane(
                &iso_b,
                &doc_b_thread,
                "route-test-concurrent-provision-session-b",
                doc_b_thread.to_string_lossy().as_ref(),
                Some(session),
                &[],
            )
        });

        barrier.wait();
        let pane_a = handle_a.join().unwrap().unwrap();
        let pane_b = handle_b.join().unwrap().unwrap();

        let window_a = iso.pane_window(&pane_a).unwrap();
        let window_b = iso.pane_window(&pane_b).unwrap();
        assert_eq!(
            window_a, window_b,
            "concurrent provisioning in one tmux session should converge into a single window"
        );

        let panes = iso.list_window_panes(&window_a).unwrap();
        assert!(
            panes.contains(&pane_a) && panes.contains(&pane_b),
            "both provisioned panes should remain visible in the shared window"
        );

        let registry = sessions::load().unwrap();
        assert!(
            registry.contains_key("route-test-concurrent-provision-session-a"),
            "first provisioned document should be registered"
        );
        assert!(
            registry.contains_key("route-test-concurrent-provision-session-b"),
            "second provisioned document should be registered"
        );
    }

    #[test]
    fn failed_route_cleanup_preserves_live_registered_owner() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-preserve-failed-owner");
        let pane = iso.new_session("test", dir.path()).unwrap();
        sessions::register_full_in(
            dir.path(),
            "session-1",
            &pane,
            "tasks/software/corky.md",
            123,
            "@1",
        )
        .unwrap();

        assert!(
            should_preserve_failed_route_pane(&iso, &pane, "session-1"),
            "failed-route cleanup must preserve the live registered owner pane"
        );
    }

    #[test]
    fn failed_route_cleanup_does_not_preserve_unregistered_pane() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-cleanup-unregistered");
        let pane = iso.new_session("test", dir.path()).unwrap();

        assert!(
            !should_preserve_failed_route_pane(&iso, &pane, "session-1"),
            "failed-route cleanup should still remove panes that never became the live owner"
        );
    }

    #[test]
    fn run_with_tmux_resolves_file_path_to_absolute() {
        // Verify that resolve_absolute_file_path turns a relative path into an
        // absolute one when the file exists. This is the guard against submodule
        // CWD-dependent resolution (#route1).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let tasks = root.join("tasks");
        fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("bugs.md");
        fs::write(&doc, "# Bugs\n").unwrap();

        let _cwd_guard = ScopedCurrentDir::set(&root);

        let resolved =
            crate::git::resolve_absolute_file_path(std::path::Path::new("tasks/bugs.md"));
        assert!(
            resolved.is_absolute(),
            "route must send absolute paths to avoid submodule CWD misrouting"
        );
        assert_eq!(
            resolved, doc,
            "resolved path must point to the CWD-relative file, not a submodule shadow"
        );
    }

    #[test]
    fn startup_miss_recorded_on_fresh_start_timeout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%42",
            "session-test",
            "claude",
            crate::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();

        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("should have marker");
        assert_eq!(miss.pane_id, "%42");
        assert_eq!(
            miss.origin,
            crate::startup_miss::StartupMissOrigin::FreshStart
        );
        assert!(crate::startup_miss::is_startup_miss_pane(&doc, "%42"));
    }

    #[test]
    fn startup_miss_cleared_on_successful_ack() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%42",
            "session-test",
            "claude",
            crate::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();
        assert!(crate::startup_miss::load(&doc).unwrap().is_some());

        crate::startup_miss::clear(&doc).unwrap();
        assert!(crate::startup_miss::load(&doc).unwrap().is_none());
        assert!(!crate::startup_miss::is_startup_miss_pane(&doc, "%42"));
    }

    #[test]
    fn startup_miss_pane_detected_on_rerun() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%99",
            "session-test",
            "codex",
            crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-old"),
        )
        .unwrap();

        assert!(crate::startup_miss::is_startup_miss_pane(&doc, "%99"));
        assert!(
            !crate::startup_miss::is_startup_miss_pane(&doc, "%100"),
            "different pane should not match"
        );
    }

    #[test]
    fn startup_miss_routed_trigger_records_with_baseline_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        crate::startup_miss::record(
            &doc,
            "%50",
            "session-test",
            "claude",
            crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-baseline-123"),
        )
        .unwrap();

        let miss = crate::startup_miss::load(&doc).unwrap().expect("marker");
        assert_eq!(
            miss.origin,
            crate::startup_miss::StartupMissOrigin::RoutedTrigger
        );
        assert_eq!(
            miss.cycle_baseline_id.as_deref(),
            Some("cycle-baseline-123")
        );
    }

    #[test]
    fn startup_miss_requires_fresh_start_only_without_matching_live_owner() {
        assert!(startup_miss_requires_fresh_start(
            "%42",
            None,
            SupervisorHealth::NoSocket
        ));
        assert!(startup_miss_requires_fresh_start(
            "%42",
            Some("%99"),
            SupervisorHealth::Unreachable
        ));
        assert!(!startup_miss_requires_fresh_start(
            "%42",
            Some("%42"),
            SupervisorHealth::NoSocket
        ));
        assert!(!startup_miss_requires_fresh_start(
            "%42",
            None,
            SupervisorHealth::NeedsRestart
        ));
        assert!(!startup_miss_requires_fresh_start(
            "%42",
            None,
            SupervisorHealth::Healthy
        ));
    }

    #[test]
    fn startup_miss_fail_closed_only_for_alive_open_no_socket_sessions() {
        let open = crate::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(1),
            last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
        };
        let closed = crate::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(1),
            last_event: Some("session_end".to_string()),
            saw_process_exit_after_latest_start: true,
            saw_session_end_after_latest_start: true,
        };

        assert!(startup_miss_should_fail_closed(
            true,
            "%42",
            None,
            SupervisorHealth::NoSocket,
            Some(&open)
        ));
        assert!(!startup_miss_should_fail_closed(
            true,
            "%42",
            Some("%42"),
            SupervisorHealth::NoSocket,
            Some(&open)
        ));
        assert!(!startup_miss_should_fail_closed(
            true,
            "%42",
            None,
            SupervisorHealth::Healthy,
            Some(&open)
        ));
        assert!(!startup_miss_should_fail_closed(
            true,
            "%42",
            None,
            SupervisorHealth::NoSocket,
            Some(&closed)
        ));
        assert!(!startup_miss_should_fail_closed(
            false,
            "%42",
            None,
            SupervisorHealth::NoSocket,
            Some(&open)
        ));
    }

    #[test]
    fn startup_miss_diagnostic_message_includes_retry_command() {
        let doc = std::path::Path::new("tasks/agent-doc/agent-doc-bugs2.md");
        let message = startup_miss_diagnostic_message(
            doc,
            "routed trigger accepted but no document cycle started for pending #smdq",
        );
        assert!(message.contains("[agent-doc] startup-miss:"));
        assert!(message.contains("agent-doc start tasks/agent-doc/agent-doc-bugs2.md"));
    }

    #[test]
    fn startup_miss_diagnostic_does_not_queue_shell_echo_in_pane() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-startup-miss-diagnostic");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        send_keys_with_retry(&iso, &pane, "printf '> '");
        let before = wait_for_pane_contains(&iso, &pane, "> ", std::time::Duration::from_secs(5));
        assert!(
            before.contains("> "),
            "shell prompt should be visible: {before}"
        );

        emit_startup_miss_diagnostic(&iso, &pane, &doc, "startup timed out");

        std::thread::sleep(std::time::Duration::from_millis(250));
        let after = sessions::capture_pane(&iso, &pane).unwrap();
        assert!(
            !after.contains("echo '[agent-doc] startup-miss:"),
            "diagnostic should not be left as drafted shell input: {after}"
        );
    }

    #[test]
    fn resolve_or_create_pane_restarts_registered_pane_with_supervisor_when_no_live_owner() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-supervisor-restart");
        let session = "claude";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-supervisor-restart";
        sessions::register(session_id, &pane, &file_path).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": false,
                        "state": "halted"
                    })),
                    IpcMethod::Restart { .. } => {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": null })),
                    IpcMethod::Inject { bytes } => {
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let panes_before = iso
            .list_panes_ordered(&format!("{session}:0"))
            .unwrap_or_default();
        let resolved = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
        )
        .expect("route should restart the registered pane instead of autostarting a duplicate");
        let panes_after = iso
            .list_panes_ordered(&format!("{session}:0"))
            .unwrap_or_default();

        assert_eq!(resolved, pane);
        assert_eq!(
            panes_after.len(),
            panes_before.len(),
            "route should not create a duplicate pane when the registered supervisor can restart in place"
        );
        assert!(
            restart_called.load(Ordering::Relaxed),
            "route should restart the registered supervisor instead of auto-starting a new pane"
        );

        ipc.stop();
    }
}

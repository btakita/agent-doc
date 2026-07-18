//! Route startup and provisioning I/O.
//!
//! A shared `agent-doc` tmux window may span nested project roots. Fresh
//! provisioning may use a pane from another root as a split-only anchor only
//! when every visible pane proves ownership of a different agent-doc document;
//! unknown ownership and same-document ownership remain fail-closed.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cycle_ack::{RouteCycleAckEffects, wait_for_start_ack};
use crate::dispatch::{RouteDispatchEffects, dispatch_routed_reopen};
use crate::dispatch_only::{
    DispatchOnlyRouteEffects, DispatchOnlySendReopenOptions, dispatch_only_send_reopen,
};
use crate::dispatch_recovery::resolve_fresh_dispatch_target_after_ready_wait;
use crate::dispatch_target::register_dispatch_target;
use crate::session_resolution::{
    ensure_auto_start_target_session, evict_previous_stash_pane, find_registered_pane_in_session,
    resolve_target_session,
};
use crate::startup_harness::resolve_harness_for_file;
use crate::startup_locks::{StartupLockAcquire, StartupLockMode, acquire_startup_locks};
use crate::startup_ready::{fresh_start_no_ack_outcome, wait_for_agent_ready};
use agent_doc_controller::dispatch::{
    DispatchOnlyReopenDelivery, DuplicatePanePolicyErrorFacts, FreshStartAckOutcome,
    RoutedDispatchStartProof, duplicate_pane_policy_error_message, fresh_route_start_ack_timeout,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::route_owned::RouteOwnedReapPolicy;
use agent_doc_tmux::is_first_column;
use tmux_router::Tmux;

#[derive(Debug, Clone, Copy)]
pub struct RouteStartupEffects {
    pub route_dispatch_effects: RouteDispatchEffects,
    pub dispatch_only_route_effects: DispatchOnlyRouteEffects,
    pub route_cycle_ack_effects: RouteCycleAckEffects,
}

/// `#jbtsiftnosub`: bounded re-verify window for the auto-start cold-start gate.
/// After `wait_for_agent_ready` reports ready, the pane should already show a
/// dispatch-ready prompt, so this is the small race window between the readiness
/// proof and the actual send; if the composer is still cold-starting past this
/// bound the auto-start dispatch fails closed instead of typing into a
/// not-yet-submit-ready composer.
const AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingStartupRegistrationDecision<'a> {
    Reuse(&'a str),
    IgnoreStale(&'a str),
    None,
}

fn decide_existing_startup_registration<'a>(
    existing_pane: Option<&'a str>,
    existing_alive: bool,
    live_owner: Option<&str>,
) -> ExistingStartupRegistrationDecision<'a> {
    let Some(existing_pane) = existing_pane else {
        return ExistingStartupRegistrationDecision::None;
    };
    if !existing_alive {
        return ExistingStartupRegistrationDecision::None;
    }
    if live_owner == Some(existing_pane) {
        ExistingStartupRegistrationDecision::Reuse(existing_pane)
    } else {
        ExistingStartupRegistrationDecision::IgnoreStale(existing_pane)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowPaneOwnerObservation<'a> {
    pane: &'a str,
    owner_document: Option<&'a Path>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingWindowAnchorDecision<'a> {
    Use(&'a str),
    NoPanes,
    RefuseUnknownOwner(&'a str),
    RefuseRequestedDocumentOwner(&'a str),
}

fn decide_existing_window_anchor<'a>(
    requested_document: &Path,
    panes: &'a [WindowPaneOwnerObservation<'a>],
    split_before: bool,
) -> ExistingWindowAnchorDecision<'a> {
    if panes.is_empty() {
        return ExistingWindowAnchorDecision::NoPanes;
    }
    for observation in panes {
        match observation.owner_document {
            Some(owner) if owner == requested_document => {
                return ExistingWindowAnchorDecision::RefuseRequestedDocumentOwner(
                    observation.pane,
                );
            }
            Some(_) => {}
            None => {
                return ExistingWindowAnchorDecision::RefuseUnknownOwner(observation.pane);
            }
        }
    }
    let selected = if split_before {
        panes.first()
    } else {
        panes.last()
    }
    .expect("non-empty pane observations were checked above");
    ExistingWindowAnchorDecision::Use(selected.pane)
}

fn pane_owner_document(tmux: &Tmux, pane: &str) -> Option<PathBuf> {
    if let Some(pane_pid) = agent_doc_tmux_io::pane_pid(tmux, pane)
        && let Some(raw_document) =
            agent_doc_process_owner_io::process_tree_agent_doc_owner_document(&pane_pid.to_string())
    {
        let raw_path = PathBuf::from(raw_document);
        let candidate = if raw_path.is_absolute() {
            raw_path
        } else {
            agent_doc_tmux_io::pane_current_path(tmux, pane)?.join(raw_path)
        };
        if let Ok(canonical) = candidate.canonicalize() {
            return Some(canonical);
        }
    }
    pane_owner_document_via_registry(tmux, pane)
}

/// `#cross-project-window-anchor` — prove pane ownership from the session
/// registry when the process tree cannot.
///
/// `process_tree_agent_doc_owner_document` only proves ownership when an
/// agent-doc document path appears in the pane's process tree. A pane running a
/// BARE harness (`claude`, `codex`) with no `.md` in its cmdline owns its
/// document through the registry instead, so the process-tree probe returns
/// `None` and [`decide_existing_window_anchor`] refuses the whole window with
/// "no provable agent-doc document owner" — even though the pane is a perfectly
/// ordinary agent-doc session. That made a SUBMODULE document unroutable
/// whenever a superproject-owned pane shared the target window, and the emitted
/// remediation told the operator to `tmux kill-pane` their own live session.
///
/// The registry is per-project, so resolve each pane against ITS OWN project
/// root (derived from the pane's working directory) rather than the requesting
/// document's. That is what makes this work across the superproject/submodule
/// boundary, which `route_startup_cross_project_window_anchor` already
/// anticipates downstream.
///
/// This only ever converts `None` into `Some`, so it is a strict widening of
/// what counts as provable: a genuinely foreign pane (a plain shell, an editor)
/// has no registry entry, still resolves to `None`, and the window is still
/// refused. The safety intent of the guard is preserved.
fn pane_owner_document_via_registry(tmux: &Tmux, pane: &str) -> Option<PathBuf> {
    let pane_cwd = agent_doc_tmux_io::pane_current_path(tmux, pane)?;
    let project_root = agent_doc_project_root_io::project_root_containing(&pane_cwd)?;
    let registry = agent_doc_session_registry_io::load_in(&project_root).ok()?;
    let candidate = registry_pane_owner_document_path(&registry, &project_root, pane)?;
    candidate.canonicalize().ok()
}

/// Pure resolution half of [`pane_owner_document_via_registry`]: pick the
/// registry entry claiming `pane` and resolve its recorded document path against
/// the owning project root. Returns `None` when no entry claims the pane or the
/// entry records no file, which is what keeps a genuinely foreign pane refused.
fn registry_pane_owner_document_path(
    registry: &tmux_router::registry::Registry,
    project_root: &Path,
    pane: &str,
) -> Option<PathBuf> {
    let entry = registry.values().find(|entry| entry.pane == pane)?;
    let file = entry.file.trim();
    if file.is_empty() {
        return None;
    }
    let raw_path = PathBuf::from(file);
    if raw_path.is_absolute() {
        Some(raw_path)
    } else {
        // Registry paths are stored relative to the owning project root.
        Some(project_root.join(raw_path))
    }
}

/// Fresh-route agent dispatch-ready wait budget (`#waitmachine2`). Historically
/// 30s; routed through the unified wait-machinery ceiling so the operator's
/// "never hang > 10s" bound is enforced in one place: the 30s request is clamped
/// to [`agent_doc_turn::wait_machine::GLOBAL_HANG_CEILING`] (10s). The underlying
/// `wait_for_agent_ready` poll loop keeps its existing fast-fail-on-dead-pane and
/// blocker-streak semantics; only the ceiling changes.
const FRESH_ROUTE_AGENT_READY_TIMEOUT: Duration =
    agent_doc_turn::wait_machine::clamp_to_ceiling(Duration::from_secs(30));

/// Editor-origin provisioning creates an interactive document session, not a
/// one-shot route worker. Layout reconciliation is identified by `skip_wait`;
/// direct Run Agent Doc requests carry the controller's editor-route attempt id.
/// Controller/watchdog recovery has neither signal and retains auto-reap.
fn route_owned_reap_policy_for_start(
    skip_wait: bool,
    editor_route_attempt_id: Option<&str>,
) -> RouteOwnedReapPolicy {
    if skip_wait || editor_route_attempt_id.is_some_and(|id| !id.trim().is_empty()) {
        RouteOwnedReapPolicy::KeepAlive
    } else {
        RouteOwnedReapPolicy::Auto
    }
}

/// Auto-start a new agent session in tmux using the default session name.
/// Public so `sync.rs` can call it for unresolved files.
///
/// `context_session` is an optional session override from the calling context
/// (e.g., the sync target session). Used when frontmatter has no `tmux_session`
/// and sync has already resolved a more specific session from editor/window
/// context.
#[allow(dead_code)]
pub fn auto_start(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    effects: RouteStartupEffects,
) -> Result<String> {
    auto_start_ext(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        false,
        false,
        effects,
    )
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
    effects: RouteStartupEffects,
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
        effects,
    )
}

/// Like [`provision_pane`], but returns `Ok(None)` instead of blocking when a
/// same-document or same-session startup is already in progress.
pub fn try_provision_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    col_args: &[String],
    effects: RouteStartupEffects,
) -> Result<Option<String>> {
    let split_before = is_first_column(file, col_args);
    auto_start_ext_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        true,
        split_before,
        StartupLockMode::Try,
        effects,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn auto_start_ext(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
    effects: RouteStartupEffects,
) -> Result<String> {
    match auto_start_ext_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        context_session,
        skip_wait,
        split_before,
        StartupLockMode::Blocking,
        effects,
    )? {
        Some(pane) => Ok(pane),
        None => anyhow::bail!("startup lock was busy in blocking startup mode"),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn auto_start_ext_with_lock_mode(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
    startup_lock_mode: StartupLockMode,
    effects: RouteStartupEffects,
) -> Result<Option<String>> {
    let harness = resolve_harness_for_file(file);
    let session_name = resolve_target_session(tmux, context_session, &[], Some(file), &harness);
    ensure_auto_start_target_session(tmux, context_session, &session_name, &harness)?;
    auto_start_in_session_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        &session_name,
        skip_wait,
        split_before,
        &harness,
        None,
        None,
        false,
        startup_lock_mode,
        effects,
    )
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
pub fn auto_start_in_session(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    session_name: &str,
    skip_wait: bool,
    split_before: bool,
    harness: &HarnessConfig,
    startup_miss_handoff_blocked_pane: Option<&str>,
    created_panes: Option<&mut Vec<String>>,
    dispatch_only: bool,
    effects: RouteStartupEffects,
) -> Result<String> {
    match auto_start_in_session_with_lock_mode(
        tmux,
        file,
        session_id,
        file_path,
        session_name,
        skip_wait,
        split_before,
        harness,
        startup_miss_handoff_blocked_pane,
        created_panes,
        dispatch_only,
        StartupLockMode::Blocking,
        effects,
    )? {
        Some(pane) => Ok(pane),
        None => anyhow::bail!("startup lock was busy in blocking startup mode"),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn auto_start_in_session_with_lock_mode(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    session_name: &str,
    skip_wait: bool,
    split_before: bool,
    harness: &HarnessConfig,
    startup_miss_handoff_blocked_pane: Option<&str>,
    mut created_panes: Option<&mut Vec<String>>,
    dispatch_only: bool,
    startup_lock_mode: StartupLockMode,
    effects: RouteStartupEffects,
) -> Result<Option<String>> {
    // Serialize auto-starts for both the document and the target tmux session.
    // This prevents duplicate starts for the same file and split-target races
    // when two different documents provision concurrently into the same window.
    let startup_locks = match acquire_startup_locks(file, session_name, startup_lock_mode)? {
        StartupLockAcquire::Acquired(locks) => locks,
        StartupLockAcquire::Busy => {
            eprintln!(
                "[route] startup lock busy for {} in session '{}' — provisioning already in progress",
                file_path, session_name
            );
            return Ok(None);
        }
    };
    let existing_registration = agent_doc_session_registry_io::lookup(session_id)?;
    let existing_alive = existing_registration
        .as_deref()
        .is_some_and(|existing| tmux.pane_alive(existing));
    let live_owner = if existing_alive {
        agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
    } else {
        None
    };
    match decide_existing_startup_registration(
        existing_registration.as_deref(),
        existing_alive,
        live_owner.as_deref(),
    ) {
        ExistingStartupRegistrationDecision::Reuse(existing) => {
            eprintln!(
                "[route] startup already provisioned live owner pane {} for {} while waiting on locks",
                existing, file_path
            );
            return Ok(Some(existing.to_string()));
        }
        ExistingStartupRegistrationDecision::IgnoreStale(existing) => {
            eprintln!(
                "[route] ignoring alive registry pane {} for {} during startup because it is not a proven live owner; creating a fresh pane",
                existing, file_path
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_startup_ignoring_unowned_registry_pane file={} pane={}",
                    file_path, existing
                ),
            );
        }
        ExistingStartupRegistrationDecision::None => {}
    }

    // Use the document's own submodule root as the pane cwd when applicable,
    // so `/agent-doc` invocations on submodule-hosted documents spawn panes
    // inside the correct submodule (e.g. `src/session-share`) instead of the
    // agent-loop super root where the command happened to be invoked from.
    let cwd = agent_doc_git_io::dirs::resolve_pane_cwd(file);
    let registry_base_dir = agent_doc_project_root_io::project_root_or_file_parent(file)
        .unwrap_or_else(|_| cwd.clone());
    let stderr_log = agent_doc_supervisor_process::start_command::route_owned_stderr_log_path(
        &registry_base_dir,
    );
    let stderr_log_dir = stderr_log
        .parent()
        .context("route-owned supervisor stderr path must include a logs directory")?;
    std::fs::create_dir_all(stderr_log_dir).with_context(|| {
        format!(
            "failed to prepare route-owned supervisor stderr directory {}",
            stderr_log_dir.display()
        )
    })?;

    // Resolve the agent-doc binary path (same binary that's currently running)
    let agent_doc_bin = agent_doc_supervisor_process::agent_doc_start_bin();

    // Try to split directly in an existing pane.
    // When skip_wait=true (sync path), prefer panes in the target window (agent-doc window)
    // over stash panes — splitting in the stash creates invisible panes.
    let window_panes = tmux
        .list_panes_ordered(&format!("{}:agent-doc", session_name))
        .unwrap_or_default();
    let registered_anchor =
        find_registered_pane_in_session(tmux, &registry_base_dir, session_name, "");
    let mut window_anchor_refusal = None;
    let existing_pane = if skip_wait {
        // Sync path: find a pane in the agent-doc window (not stash)
        let positional = if split_before {
            window_panes.first().cloned() // leftmost by screen position
        } else {
            window_panes.last().cloned() // rightmost by screen position
        };
        positional.or(registered_anchor)
    } else if registered_anchor.is_some() {
        registered_anchor
    } else if window_panes.is_empty() {
        None
    } else {
        let requested_document = file.canonicalize().ok();
        let owner_documents: Vec<Option<PathBuf>> = window_panes
            .iter()
            .map(|pane| pane_owner_document(tmux, pane))
            .collect();
        let observations: Vec<WindowPaneOwnerObservation<'_>> = window_panes
            .iter()
            .zip(&owner_documents)
            .map(|(pane, owner_document)| WindowPaneOwnerObservation {
                pane,
                owner_document: owner_document.as_deref(),
            })
            .collect();
        match requested_document.as_deref() {
            Some(requested_document) => {
                match decide_existing_window_anchor(requested_document, &observations, split_before)
                {
                    ExistingWindowAnchorDecision::Use(anchor) => {
                        let owner = observations
                            .iter()
                            .find(|observation| observation.pane == anchor)
                            .and_then(|observation| observation.owner_document)
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "route_startup_cross_project_window_anchor file={} anchor={} anchor_owner={} session={}",
                                file_path, anchor, owner, session_name
                            ),
                        );
                        Some(anchor.to_string())
                    }
                    ExistingWindowAnchorDecision::RefuseUnknownOwner(pane) => {
                        window_anchor_refusal = Some(format!(
                            "pane {pane} in the target 'agent-doc' window has no provable agent-doc document owner"
                        ));
                        None
                    }
                    ExistingWindowAnchorDecision::RefuseRequestedDocumentOwner(pane) => {
                        window_anchor_refusal = Some(format!(
                            "pane {pane} appears to own the requested document but has no authoritative registration"
                        ));
                        None
                    }
                    ExistingWindowAnchorDecision::NoPanes => None,
                }
            }
            None => {
                window_anchor_refusal = Some(format!(
                    "the requested document path {} could not be canonicalized for split-anchor ownership proof",
                    file.display()
                ));
                None
            }
        }
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
                anyhow::bail!(
                    "{}",
                    duplicate_pane_policy_error_message(DuplicatePanePolicyErrorFacts {
                        session_name,
                        file_path,
                        anchor_pane: Some(target),
                        cause: &format!("split-window failed alongside pane {} ({})", target, e),
                    })
                );
            }
        }
    } else {
        let has_agent_doc_window =
            agent_doc_tmux_io::has_window_named(tmux, session_name, "agent-doc");
        if has_agent_doc_window {
            anyhow::bail!(
                "{}",
                duplicate_pane_policy_error_message(DuplicatePanePolicyErrorFacts {
                    session_name,
                    file_path,
                    anchor_pane: None,
                    cause: window_anchor_refusal.as_deref().unwrap_or(
                        "the target session already has an 'agent-doc' window but no safe registered anchor pane was found",
                    ),
                })
            );
        } else {
            eprintln!(
                "[route] no registered pane found in session '{}', creating new window",
                session_name
            );
            tmux.auto_start(session_name, &cwd)?
        }
    };
    tmux.enable_remain_on_exit(&new_pane)?;
    if let Some(created) = created_panes.as_mut() {
        created.push(new_pane.clone());
    }

    evict_previous_stash_pane(tmux, session_id, &new_pane, session_name, harness);

    // Register immediately so subsequent route calls find this pane
    register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
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
    let start_path = agent_doc_fs::rewrite_start_path(file, &cwd, file_path);

    // Start agent-doc in the new pane. Editor-selected documents are durable
    // interactive sessions; controller/watchdog recovery remains one-shot/auto.
    let reap_policy = route_owned_reap_policy_for_start(
        skip_wait,
        agent_doc_controller_io::route_snapshot::editor_route_attempt_id().as_deref(),
    );
    let start_cmd = agent_doc_supervisor_process::start_command::route_owned_start_command_with_reap_policy_and_stderr_log(
        &agent_doc_bin,
        Path::new(&start_path),
        reap_policy,
        &stderr_log,
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_owned_start_policy file={} pane={} policy={} editor_origin={} skip_wait={}",
            file.display(),
            new_pane,
            reap_policy.as_str(),
            reap_policy == RouteOwnedReapPolicy::KeepAlive,
            skip_wait,
        ),
    );
    agent_doc_tmux_io::input_diag::log_text_submit(
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "route.auto_start",
        &format!("pane:{new_pane}"),
        &start_cmd,
        Some(&harness.binary),
        "route_owned_start_enter",
        "Enter",
    );
    agent_doc_tmux_io::send_submitted_text_logged(
        tmux,
        &new_pane,
        &start_cmd,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
        "sessions.send_submitted_text",
    )?;

    eprintln!(
        "[route] Started {} for {} in pane {} (session {})",
        harness.binary,
        file_path,
        new_pane,
        &session_id[..std::cmp::min(8, session_id.len())]
    );

    let cycle_baseline =
        agent_doc_cycle_state_io::load_with_closeout_projection(file).unwrap_or(None);

    if skip_wait {
        eprintln!(
            "[route] skip_wait=true — pane created, {} starting (sync path)",
            harness.binary
        );
    } else {
        eprintln!("[route] Waiting for {} to initialize...", harness.binary);
        let ready = wait_for_agent_ready(tmux, &new_pane, FRESH_ROUTE_AGENT_READY_TIMEOUT, harness);
        // Fresh-start recovery can clear the early geometry-only binding while
        // the harness is still booting. Re-validate the registration before we
        // dispatch, but keep the deliberately created fresh pane authoritative
        // for same-document rebind churn instead of treating it as disposable.
        let dispatch_pane = resolve_fresh_dispatch_target_after_ready_wait(
            tmux,
            session_id,
            &new_pane,
            file_path,
            startup_miss_handoff_blocked_pane,
        )?;
        if dispatch_pane != new_pane {
            eprintln!(
                "[route] fresh start pane {} handed ownership for {} back to existing pane {} during startup; dispatching follow-up there",
                new_pane, file_path, dispatch_pane
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "fresh_route_dispatch_handoff file={} fresh_pane={} dispatch_pane={} harness={}",
                    file.display(),
                    new_pane,
                    dispatch_pane,
                    harness.binary
                ),
            );
            if let Err(e) = tmux.select_pane(&dispatch_pane) {
                eprintln!(
                    "[route] warning: failed to focus handoff pane {}: {}",
                    dispatch_pane, e
                );
            }
        }
        let dispatch_start = if ready {
            eprintln!("[route] {} is ready, sending command", harness.binary);
            if dispatch_only {
                dispatch_only_send_reopen(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    DispatchOnlySendReopenOptions {
                        delivery: DispatchOnlyReopenDelivery::SupervisorIpcOnce,
                        queue_prompt_text: None,
                        effects: effects.dispatch_only_route_effects,
                    },
                )?;
                RoutedDispatchStartProof::CommandAcceptedOnly
            } else {
                // #jbtsiftnosub: close the cold-start race. `wait_for_agent_ready`
                // can prove a transient dispatch-ready prompt while the harness TUI
                // is still coming up; re-verify immediately before the send that the
                // composer is actually submit-ready, and fail closed (logging
                // `dispatch_into_starting_pane`) rather than typing the trigger into
                // a not-yet-ready composer.
                crate::startup_ready::reverify_auto_start_dispatch_ready(
                    tmux,
                    file,
                    &dispatch_pane,
                    file_path,
                    harness,
                    AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT,
                )?;
                dispatch_routed_reopen(
                    tmux,
                    file,
                    &dispatch_pane,
                    file_path,
                    harness,
                    effects.route_dispatch_effects,
                )?
            }
        } else {
            eprintln!(
                "[route] Timed out waiting for {} prompt; attempting one fallback trigger injection before failing closed",
                harness.binary
            );
            let dispatch_result = if dispatch_only {
                dispatch_only_send_reopen(
                    tmux,
                    file,
                    session_id,
                    &dispatch_pane,
                    file_path,
                    harness,
                    DispatchOnlySendReopenOptions {
                        delivery: DispatchOnlyReopenDelivery::SupervisorIpcOnce,
                        queue_prompt_text: None,
                        effects: effects.dispatch_only_route_effects,
                    },
                )
                .map(|_| RoutedDispatchStartProof::CommandAcceptedOnly)
            } else {
                dispatch_routed_reopen(
                    tmux,
                    file,
                    &dispatch_pane,
                    file_path,
                    harness,
                    effects.route_dispatch_effects,
                )
            };
            match dispatch_result {
                Ok(proof) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_recovered file={} pane={} harness={}",
                            file.display(),
                            dispatch_pane,
                            harness.binary
                        ),
                    );
                    eprintln!(
                        "[route] Fallback trigger injection recovered the fresh {} start for {}",
                        harness.binary, file_path
                    );
                    proof
                }
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "fresh_route_trigger_missing file={} pane={} harness={}",
                            file.display(),
                            new_pane,
                            harness.binary
                        ),
                    );
                    return Err(err);
                }
            }
        };

        if dispatch_only {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "fresh_route_dispatch_only file={} pane={} harness={}",
                    file.display(),
                    dispatch_pane,
                    harness.binary
                ),
            );
            let _ = dispatch_start;
            return Ok(Some(dispatch_pane));
        }

        let ack_timeout = fresh_route_start_ack_timeout(cfg!(test));
        match wait_for_start_ack(file, cycle_baseline.as_ref(), ack_timeout) {
            Some(state) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "fresh_route_start_acknowledged file={} pane={} harness={} cycle={} phase={} timeout_secs={}",
                        file.display(),
                        dispatch_pane,
                        harness.binary,
                        state.cycle_id,
                        match state.phase {
                            agent_doc_turn::CyclePhase::PreflightStarted => "preflight_started",
                            agent_doc_turn::CyclePhase::ResponseCaptured => "response_captured",
                            agent_doc_turn::CyclePhase::WriteApplied => "write_applied",
                            agent_doc_turn::CyclePhase::Committed => "committed",
                            agent_doc_turn::CyclePhase::Abandoned => "abandoned",
                        },
                        ack_timeout.as_secs()
                    ),
                );
                let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
            }
            None => {
                // (#jbtsiftnosub2) Classify the no-ack pane from a single
                // capture. A dispatch-ready pane whose composer STILL shows the
                // injected trigger unsubmitted is the JB-created-fresh-pane
                // "prompt added but not submitted" drift — resubmit it once
                // before deciding, instead of misreading a stranded request as a
                // legitimate idle no-op.
                let trigger = harness.trigger_command(file_path);
                let mut outcome =
                    fresh_start_no_ack_outcome(tmux, &dispatch_pane, harness, &trigger);
                if matches!(outcome, FreshStartAckOutcome::StrandedTriggerResubmit) {
                    outcome = resubmit_stranded_fresh_start_trigger(
                        tmux,
                        file,
                        &dispatch_pane,
                        harness,
                        &trigger,
                        cycle_baseline.as_ref(),
                        ack_timeout,
                    );
                }
                match outcome {
                    FreshStartAckOutcome::CycleAcknowledged => {
                        // The resubmit landed a document cycle.
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "fresh_route_start_acknowledged_after_resubmit file={} pane={} harness={} timeout_secs={} #jbtsiftnosub2",
                                file.display(),
                                dispatch_pane,
                                harness.binary,
                                ack_timeout.as_secs()
                            ),
                        );
                        let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
                    }
                    FreshStartAckOutcome::IdleNoOpKeep => {
                        // (#route-reaps-idle-fresh-start) The trigger was proven
                        // dispatched above, and the pane has returned to a
                        // dispatch-ready prompt with an empty composer: the first
                        // cycle was a legitimate no-op (empty/halted queue,
                        // preflight `no_changes`) — there was simply nothing to
                        // acknowledge. Keep the live idle session instead of
                        // reaping a healthy start (the "I cannot start
                        // lazily-rs.md, killed immediately" symptom).
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "fresh_route_start_idle_no_op file={} pane={} harness={} timeout_secs={} note=trigger dispatched, pane dispatch-ready, no-op first cycle kept as idle session",
                                file.display(),
                                dispatch_pane,
                                harness.binary,
                                ack_timeout.as_secs()
                            ),
                        );
                        eprintln!(
                            "[route] fresh {} start for {} produced a no-op first cycle (nothing queued); pane {} is idle and dispatch-ready — keeping the live idle session",
                            harness.binary,
                            file.display(),
                            dispatch_pane
                        );
                        let _ = agent_doc_supervisor_io::startup_miss::clear_startup_miss(file);
                    }
                    FreshStartAckOutcome::StrandedTriggerResubmit
                    | FreshStartAckOutcome::GenuineMissReap => {
                        // Genuine miss: pane never ready / hung, or the trigger is
                        // still stuck unsubmitted even after a resubmit attempt.
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "fresh_route_start_missing file={} pane={} harness={} timeout_secs={}",
                                file.display(),
                                dispatch_pane,
                                harness.binary,
                                ack_timeout.as_secs()
                            ),
                        );
                        let baseline_id = cycle_baseline.as_ref().map(|b| b.cycle_id.as_str());
                        let _ = agent_doc_supervisor_io::startup_miss::record_startup_miss(
                            file,
                            &dispatch_pane,
                            session_id,
                            &harness.binary,
                            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
                            baseline_id,
                        );
                        (effects.route_cycle_ack_effects.emit_startup_miss_diagnostic)(
                            tmux,
                            &dispatch_pane,
                            file,
                            &format!(
                                "fresh start: trigger {} but no document cycle started",
                                dispatch_start.dispatch_stage_label()
                            ),
                        );
                        anyhow::bail!(
                            "fresh {} start for {} never acknowledged with a document cycle after trigger {}",
                            harness.binary,
                            file.display(),
                            dispatch_start.startup_miss_label()
                        );
                    }
                }
            }
        }
    }

    let final_pane = if skip_wait {
        register_dispatch_target(tmux, session_id, &new_pane, file_path)?;
        new_pane
    } else {
        resolve_fresh_dispatch_target_after_ready_wait(
            tmux,
            session_id,
            &new_pane,
            file_path,
            startup_miss_handoff_blocked_pane,
        )?
    };
    let _ = file; // suppress unused warning
    Ok(Some(final_pane))
}

/// (#jbtsiftnosub2) Resubmit a fresh-start trigger that the harness composer
/// typed but never submitted, then re-classify the pane.
///
/// Sends one bare harness submit key (`Enter`) to the stranded composer draft
/// and waits a bounded ack window. Returns `CycleAcknowledged` when the resubmit
/// finally starts a document cycle; otherwise re-captures the pane and returns
/// the fresh classification (`IdleNoOpKeep` if the composer cleared into a
/// genuine no-op, or `StrandedTriggerResubmit`/`GenuineMissReap` if the trigger
/// is still stuck), so the caller records a startup-miss and fails closed
/// instead of silently keeping the operator's request unsubmitted.
#[allow(clippy::too_many_arguments)]
fn resubmit_stranded_fresh_start_trigger(
    tmux: &Tmux,
    file: &Path,
    dispatch_pane: &str,
    harness: &HarnessConfig,
    trigger: &str,
    cycle_baseline: Option<&agent_doc_cycle_state_io::CycleState>,
    ack_timeout: Duration,
) -> FreshStartAckOutcome {
    let submit_key = agent_doc_tmux_commands::tmux_submit_key_for_harness(&harness.binary);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "fresh_route_stranded_trigger_resubmit file={} pane={} harness={} submit_key={} #jbtsiftnosub2 note=trigger typed into composer but never submitted; resending submit key",
            file.display(),
            dispatch_pane,
            harness.binary,
            submit_key
        ),
    );
    eprintln!(
        "[route] fresh {} start for {} left the trigger unsubmitted in the composer; resending {} to submit",
        harness.binary,
        file.display(),
        submit_key
    );
    if let Err(e) = agent_doc_tmux_io::send_key_logged(
        tmux,
        dispatch_pane,
        submit_key,
        agent_doc_tmux_io::input_diag::InputDiagSink::new(Some(file), agent_doc_ops_log_io::log_op),
        "route.stranded_trigger_resubmit",
    ) {
        eprintln!(
            "[route] warning: failed to resend submit key to stranded pane {}: {}",
            dispatch_pane, e
        );
        return FreshStartAckOutcome::GenuineMissReap;
    }
    match wait_for_start_ack(file, cycle_baseline, ack_timeout) {
        Some(_) => FreshStartAckOutcome::CycleAcknowledged,
        None => fresh_start_no_ack_outcome(tmux, dispatch_pane, harness, trigger),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_origin_startup_is_keep_alive() {
        assert_eq!(
            route_owned_reap_policy_for_start(false, Some("editor-attempt")),
            RouteOwnedReapPolicy::KeepAlive
        );
        assert_eq!(
            route_owned_reap_policy_for_start(true, None),
            RouteOwnedReapPolicy::KeepAlive
        );
    }

    #[test]
    fn controller_recovery_startup_retains_auto_reap() {
        assert_eq!(
            route_owned_reap_policy_for_start(false, None),
            RouteOwnedReapPolicy::Auto
        );
        assert_eq!(
            route_owned_reap_policy_for_start(false, Some("  ")),
            RouteOwnedReapPolicy::Auto
        );
    }

    #[test]
    fn startup_registration_decision_reuses_matching_live_owner() {
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), true, Some("%7")),
            ExistingStartupRegistrationDecision::Reuse("%7")
        );
    }

    #[test]
    fn startup_registration_decision_ignores_alive_unowned_registry_pane() {
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), true, None),
            ExistingStartupRegistrationDecision::IgnoreStale("%7")
        );
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), true, Some("%9")),
            ExistingStartupRegistrationDecision::IgnoreStale("%7")
        );
    }

    #[test]
    fn startup_registration_decision_ignores_missing_or_dead_registration() {
        assert_eq!(
            decide_existing_startup_registration(None, false, None),
            ExistingStartupRegistrationDecision::None
        );
        assert_eq!(
            decide_existing_startup_registration(Some("%7"), false, Some("%7")),
            ExistingStartupRegistrationDecision::None
        );
    }

    #[test]
    fn cross_project_window_anchor_uses_requested_edge_when_all_owners_differ() {
        let requested = Path::new("/repo/child/tasks/requested.md");
        let root_left = Path::new("/repo/tasks/left.md");
        let root_right = Path::new("/repo/tasks/right.md");
        let panes = [
            WindowPaneOwnerObservation {
                pane: "%14",
                owner_document: Some(root_left),
            },
            WindowPaneOwnerObservation {
                pane: "%15",
                owner_document: Some(root_right),
            },
        ];

        assert_eq!(
            decide_existing_window_anchor(requested, &panes, true),
            ExistingWindowAnchorDecision::Use("%14")
        );
        assert_eq!(
            decide_existing_window_anchor(requested, &panes, false),
            ExistingWindowAnchorDecision::Use("%15")
        );
    }

    #[test]
    fn cross_project_window_anchor_refuses_requested_document_owner() {
        let requested = Path::new("/repo/child/tasks/requested.md");
        let panes = [WindowPaneOwnerObservation {
            pane: "%9",
            owner_document: Some(requested),
        }];

        assert_eq!(
            decide_existing_window_anchor(requested, &panes, false),
            ExistingWindowAnchorDecision::RefuseRequestedDocumentOwner("%9")
        );
    }

    #[test]
    fn cross_project_window_anchor_refuses_unknown_owner() {
        let requested = Path::new("/repo/child/tasks/requested.md");
        let panes = [WindowPaneOwnerObservation {
            pane: "%9",
            owner_document: None,
        }];

        assert_eq!(
            decide_existing_window_anchor(requested, &panes, false),
            ExistingWindowAnchorDecision::RefuseUnknownOwner("%9")
        );
    }

    fn registry_entry(pane: &str, file: &str) -> tmux_router::registry::RegistryEntry {
        tmux_router::registry::RegistryEntry {
            pane: pane.to_string(),
            pid: 0,
            cwd: String::new(),
            started: String::new(),
            session_id: String::new(),
            file: file.to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        }
    }

    // `#cross-project-window-anchor`: a pane running a BARE harness (`claude`)
    // has no document in its process tree, so the process-tree probe cannot
    // prove ownership. The registry can, and that is what stops a submodule
    // document from being refused because a superproject pane shares the window.
    #[test]
    fn registry_proves_bare_harness_pane_owner_document() {
        let mut registry = tmux_router::registry::Registry::new();
        registry.insert(
            "session-a".to_string(),
            registry_entry("%23", "tasks/agent-doc/agent-doc-bugs2.md"),
        );

        assert_eq!(
            registry_pane_owner_document_path(&registry, Path::new("/repo"), "%23"),
            Some(PathBuf::from("/repo/tasks/agent-doc/agent-doc-bugs2.md"))
        );
    }

    #[test]
    fn registry_pane_owner_resolves_absolute_entry_path_as_is() {
        let mut registry = tmux_router::registry::Registry::new();
        registry.insert(
            "session-a".to_string(),
            registry_entry("%23", "/elsewhere/tasks/doc.md"),
        );

        assert_eq!(
            registry_pane_owner_document_path(&registry, Path::new("/repo"), "%23"),
            Some(PathBuf::from("/elsewhere/tasks/doc.md"))
        );
    }

    // The widening must stay strict: a pane no registry entry claims is still
    // unprovable, so `decide_existing_window_anchor` still refuses the window.
    // Without this, the guard would happily split into a plain shell or editor.
    #[test]
    fn registry_does_not_prove_unclaimed_pane() {
        let mut registry = tmux_router::registry::Registry::new();
        registry.insert("session-a".to_string(), registry_entry("%14", "tasks/a.md"));

        assert_eq!(
            registry_pane_owner_document_path(&registry, Path::new("/repo"), "%99"),
            None
        );
    }

    #[test]
    fn registry_does_not_prove_entry_without_file() {
        let mut registry = tmux_router::registry::Registry::new();
        registry.insert("session-a".to_string(), registry_entry("%23", "   "));

        assert_eq!(
            registry_pane_owner_document_path(&registry, Path::new("/repo"), "%23"),
            None
        );
    }
}

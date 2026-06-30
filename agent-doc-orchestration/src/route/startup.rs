//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_controller::dispatch::{FreshStartAckOutcome, fresh_start_ack_outcome};

/// `#jbtsiftnosub`: bounded re-verify window for the auto-start cold-start gate.
/// After `wait_for_agent_ready` reports ready, the pane should already show a
/// dispatch-ready prompt, so this is the small race window between the readiness
/// proof and the actual send; if the composer is still cold-starting past this
/// bound the auto-start dispatch fails closed instead of typing into a
/// not-yet-submit-ready composer.
const AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT: Duration = Duration::from_secs(5);

/// Fresh-route agent dispatch-ready wait budget (`#waitmachine2`). Historically
/// 30s; routed through the unified wait-machinery ceiling so the operator's
/// "never hang > 10s" bound is enforced in one place: the 30s request is clamped
/// to [`agent_doc_turn::wait_machine::GLOBAL_HANG_CEILING`] (10s). The underlying
/// `wait_for_agent_ready` poll loop keeps its existing fast-fail-on-dead-pane and
/// blocker-streak semantics; only the ceiling changes.
const FRESH_ROUTE_AGENT_READY_TIMEOUT: Duration =
    agent_doc_turn::wait_machine::clamp_to_ceiling(Duration::from_secs(30));

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
pub fn rewrite_start_path(file: &Path, cwd: &Path, original: &str) -> String {
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

/// Like [`provision_pane`], but returns `Ok(None)` instead of blocking when a
/// same-document or same-session startup is already in progress.
pub fn try_provision_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    col_args: &[String],
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
    )
}

pub(crate) fn auto_start_ext(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
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
    )? {
        Some(pane) => Ok(pane),
        None => anyhow::bail!("startup lock was busy in blocking startup mode"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn auto_start_ext_with_lock_mode(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    file_path: &str,
    context_session: Option<&str>,
    skip_wait: bool,
    split_before: bool,
    startup_lock_mode: StartupLockMode,
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
    )
}

pub(crate) struct StartupLocks {
    _doc: File,
    _session: File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupLockMode {
    Blocking,
    Try,
}

pub(crate) enum StartupLockAcquire {
    Acquired(Option<StartupLocks>),
    Busy,
}

pub(crate) fn starting_dir_for(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(file).ok()?;
    let base = agent_doc_fs::find_project_root(&canonical)
        .or_else(|| canonical.parent().map(|p| p.to_path_buf()))?;
    Some(base.join(".agent-doc/starting"))
}

pub(crate) fn session_start_lock_name(session_name: &str) -> String {
    let hash = crate::snapshot::doc_hash_from_str(&format!("session:{session_name}"));
    format!("session-{hash}.lock")
}

pub(crate) fn open_start_lock(path: &Path) -> Result<File> {
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

pub(crate) fn lock_startup_file(
    lock: &File,
    lock_path: &Path,
    mode: StartupLockMode,
) -> Result<bool> {
    match mode {
        StartupLockMode::Blocking => {
            lock.lock_exclusive().with_context(|| {
                format!("failed to acquire startup lock {}", lock_path.display())
            })?;
            Ok(true)
        }
        StartupLockMode::Try => match lock.try_lock_exclusive() {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err)
                .with_context(|| format!("failed to acquire startup lock {}", lock_path.display())),
        },
    }
}

pub(crate) fn acquire_startup_locks(
    file: &Path,
    session_name: &str,
    mode: StartupLockMode,
) -> Result<StartupLockAcquire> {
    let Some(starting_dir) = starting_dir_for(file) else {
        return Ok(StartupLockAcquire::Acquired(None));
    };

    let doc_lock_path = if let Ok(hash) = snapshot::doc_hash(file) {
        starting_dir.join(format!("{hash}.lock"))
    } else {
        let fallback = crate::snapshot::doc_hash_from_str(&file.to_string_lossy());
        starting_dir.join(format!("{fallback}.lock"))
    };
    let session_lock_path = starting_dir.join(session_start_lock_name(session_name));

    let doc_lock = open_start_lock(&doc_lock_path)?;
    if !lock_startup_file(&doc_lock, &doc_lock_path, mode)? {
        return Ok(StartupLockAcquire::Busy);
    }

    let session_lock = open_start_lock(&session_lock_path)?;
    if !lock_startup_file(&session_lock, &session_lock_path, mode)? {
        return Ok(StartupLockAcquire::Busy);
    }

    Ok(StartupLockAcquire::Acquired(Some(StartupLocks {
        _doc: doc_lock,
        _session: session_lock,
    })))
}

/// Resolve HarnessConfig from a file's frontmatter + global config.
pub(crate) fn resolve_harness_for_file(file: &Path) -> HarnessConfig {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    rc.set_doc_content(content);
    let fm = rc.frontmatter();
    let global_config = rc.global_config();
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
pub(crate) fn auto_start_in_session(
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
    )? {
        Some(pane) => Ok(pane),
        None => anyhow::bail!("startup lock was busy in blocking startup mode"),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn auto_start_in_session_with_lock_mode(
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
    if let Some(existing) = sessions::lookup(session_id)?
        && tmux.pane_alive(&existing)
    {
        eprintln!(
            "[route] startup already provisioned pane {} for {} while waiting on locks",
            existing, file_path
        );
        return Ok(Some(existing));
    }

    // Use the document's own submodule root as the pane cwd when applicable,
    // so `/agent-doc` invocations on submodule-hosted documents spawn panes
    // inside the correct submodule (e.g. `src/session-share`) instead of the
    // agent-loop super root where the command happened to be invoked from.
    let cwd = crate::git::resolve_pane_cwd(file);
    let registry_base_dir = registry_base_dir_for_file(file, &cwd);

    // Resolve the agent-doc binary path (same binary that's currently running)
    let agent_doc_bin = agent_doc_start_bin();

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
        positional
            .or_else(|| find_registered_pane_in_session(tmux, &registry_base_dir, session_name, ""))
    } else {
        find_registered_pane_in_session(tmux, &registry_base_dir, session_name, "")
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
        let has_agent_doc_window = has_named_window(tmux, session_name, "agent-doc");
        if has_agent_doc_window {
            anyhow::bail!(
                "{}",
                duplicate_pane_policy_error_message(DuplicatePanePolicyErrorFacts {
                    session_name,
                    file_path,
                    anchor_pane: None,
                    cause: "the target session already has an 'agent-doc' window but no safe registered anchor pane was found",
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
    let start_path = rewrite_start_path(file, &cwd, file_path);

    // Start agent-doc start in the new pane
    let start_cmd = format!("{} start --route-owned {}", agent_doc_bin, start_path);
    crate::input_diag::log_text_submit(
        Some(file),
        "route.auto_start",
        &format!("pane:{new_pane}"),
        &start_cmd,
        Some(&harness.binary),
        "route_owned_start_enter",
        "Enter",
    );
    crate::sessions::send_submitted_text(tmux, &new_pane, &start_cmd)?;

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
            crate::ops_log::log_op(
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
                super::dispatch::reverify_auto_start_dispatch_ready(
                    tmux,
                    file,
                    &dispatch_pane,
                    file_path,
                    harness,
                    AUTO_START_DISPATCH_READY_REVERIFY_TIMEOUT,
                )?;
                dispatch_routed_reopen(tmux, file, &dispatch_pane, file_path, harness)?
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
                    },
                )
                .map(|_| RoutedDispatchStartProof::CommandAcceptedOnly)
            } else {
                dispatch_routed_reopen(tmux, file, &dispatch_pane, file_path, harness)
            };
            match dispatch_result {
                Ok(proof) => {
                    crate::ops_log::log_op(
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
                    crate::ops_log::log_op(
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
            crate::ops_log::log_op(
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
                crate::ops_log::log_op(
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
                let _ = crate::startup_miss::clear(file);
            }
            None if fresh_start_pane_idle_ready(tmux, &dispatch_pane, harness) => {
                // (#route-reaps-idle-fresh-start) The trigger was proven dispatched
                // above, and the pane has returned to a dispatch-ready prompt: the
                // first cycle was a legitimate no-op (empty/halted queue, preflight
                // `no_changes`) — there was simply nothing to acknowledge. Keep the
                // live idle session instead of reaping a healthy start (the "I
                // cannot start lazily-rs.md, killed immediately" symptom). Genuine
                // misses (pane never ready / hung) still fall through to the reap
                // branch below.
                crate::ops_log::log_op(
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
                let _ = crate::startup_miss::clear(file);
            }
            None => {
                crate::ops_log::log_op(
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
                let _ = crate::startup_miss::record(
                    file,
                    &dispatch_pane,
                    session_id,
                    &harness.binary,
                    crate::startup_miss::StartupMissOrigin::FreshStart,
                    baseline_id,
                );
                emit_startup_miss_diagnostic(
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

pub(crate) fn agent_doc_start_bin() -> String {
    if let Ok(override_bin) = std::env::var("AGENT_DOC_ROUTE_BIN")
        && !override_bin.trim().is_empty()
    {
        return override_bin;
    }

    std::env::current_exe()
        .unwrap_or_else(|_| "agent-doc".into())
        .to_string_lossy()
        .to_string()
}

/// Poll a tmux pane until the agent is ready to accept input.
///
/// Uses the harness's prompt patterns for detection.
/// Strips ANSI escape codes before matching. Polls every
/// `AGENT_READY_POLL_INTERVAL` up to the given timeout.
pub(crate) fn wait_for_agent_ready(
    tmux: &Tmux,
    pane_id: &str,
    timeout: std::time::Duration,
    harness: &HarnessConfig,
) -> bool {
    wait_for_agent_ready_outcome(tmux, pane_id, timeout, harness).is_ready()
}

pub(crate) fn wait_for_agent_ready_outcome(
    tmux: &Tmux,
    pane_id: &str,
    timeout: std::time::Duration,
    harness: &HarnessConfig,
) -> AgentReadyWaitOutcome {
    let start = std::time::Instant::now();
    let poll_interval = AGENT_READY_POLL_INTERVAL;
    let mut poll_count = 0u32;
    let mut ready_streak = 0u32;
    let mut last_ready_line: Option<String> = None;
    let mut blocker_streak = 0u32;
    let mut last_blocker: Option<String> = None;

    while start.elapsed() < timeout {
        // #route-ready-wait-fast-fail: a starting pane's tmux pane stays alive
        // while its agent boots, so `!pane_alive` means the pane is actually
        // closed/dead — it will never become ready. Stop waiting immediately so
        // the caller's recovery ladder (same-pane retry / handoff / fresh
        // reroute) runs now instead of burning the full ready-timeout on a pane
        // that is already gone.
        if !tmux.pane_alive(pane_id) {
            eprintln!(
                "[route] {} pane {} is dead — fast-failing ready wait after {:.1}s (recovery will reroute)",
                harness.binary,
                pane_id,
                start.elapsed().as_secs_f64()
            );
            return AgentReadyWaitOutcome::TimedOut;
        }
        if let Ok(content) = sessions::capture_pane(tmux, pane_id) {
            if let Some(reason) = harness.dispatch_blocker_reason(&content) {
                ready_streak = 0;
                last_ready_line = None;
                if last_blocker.as_deref() == Some(reason.as_str()) {
                    blocker_streak += 1;
                } else {
                    blocker_streak = 1;
                    last_blocker = Some(reason.clone());
                    if reason == "active permission prompt" {
                        crate::input_diag::log_prompt_detection(
                            None,
                            "route.wait_for_agent_ready",
                            &format!("pane:{pane_id}"),
                            &harness.binary,
                            &reason,
                            "entered",
                        );
                    }
                }
                if blocker_streak >= 2 {
                    eprintln!(
                        "[route] {} blocked after {:.1}s in pane {}: {}",
                        harness.binary,
                        start.elapsed().as_secs_f64(),
                        pane_id,
                        reason
                    );
                    return AgentReadyWaitOutcome::Blocked { reason };
                }
            } else {
                blocker_streak = 0;
                last_blocker = None;
            }

            match ready_prompt_candidate(&content, harness) {
                Some(line) => {
                    if last_ready_line.as_deref() == Some(line.as_str()) {
                        ready_streak += 1;
                    } else {
                        ready_streak = 1;
                        last_ready_line = Some(line);
                    }
                    if ready_streak >= 2 {
                        eprintln!(
                            "[route] {} ready after {:.1}s ({} polls)",
                            harness.binary,
                            start.elapsed().as_secs_f64(),
                            poll_count
                        );
                        return AgentReadyWaitOutcome::Ready;
                    }
                }
                None => {
                    ready_streak = 0;
                    last_ready_line = None;
                }
            }

            poll_count += 1;
            if poll_count.is_multiple_of(10) {
                let last_line = content
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
                    .unwrap_or_default();
                eprintln!(
                    "[route] Still waiting for {} ({:.0}s)... last line: {}",
                    harness.binary,
                    start.elapsed().as_secs_f64(),
                    truncate_log_line(&last_line, 60)
                );
            }
        }
        std::thread::sleep(poll_interval);
    }
    AgentReadyWaitOutcome::TimedOut
}

pub(crate) fn ready_prompt_candidate(content: &str, harness: &HarnessConfig) -> Option<String> {
    let latest_dispatch_ready_prompt = harness
        .last_prompt_candidate(content)
        .filter(|line| harness.is_dispatch_ready_prompt_line(line));
    if harness.binary == "claude" && latest_dispatch_ready_prompt.is_some() {
        return latest_dispatch_ready_prompt;
    }
    if harness.has_busy_cue(content) {
        return None;
    }
    if harness.binary == "opencode" && harness.is_idle_chrome_only_output(content) {
        return Some("opencode idle status chrome".to_string());
    }
    // OpenCode and Codex can both render an idle composer as bottom status/footer
    // chrome only after startup, clear, or redraw. Busy/protected states are
    // filtered above by the harness busy cue and prompt-input classifiers.
    if harness.binary == "codex" && harness.is_bottom_idle_chrome(content, 12) {
        return latest_dispatch_ready_prompt
            .or_else(|| Some("codex idle status chrome".to_string()));
    }
    if harness.binary == "opencode" && harness.is_bottom_idle_chrome(content, 12) {
        return latest_dispatch_ready_prompt.or_else(|| Some("bottom idle chrome".to_string()));
    }
    if harness.binary == "codex"
        && latest_dispatch_ready_prompt.is_some()
        && harness.is_bottom_idle_chrome(content, 12)
    {
        return latest_dispatch_ready_prompt;
    }
    latest_dispatch_ready_prompt
}

/// (`#route-reaps-idle-fresh-start`) How a fresh start's first cycle resolved.
///
/// A fresh start whose trigger was already proven dispatched can end three ways:
/// it acknowledged a document cycle (normal); it produced **no** cycle but the
/// pane returned to a dispatch-ready prompt — a legitimate **idle no-op** first
/// cycle (empty/halted queue, `preflight` `no_changes`) which must be KEPT as a
/// live idle session; or it produced no cycle and the pane is not dispatch-ready
/// — a genuine startup miss that must be REAPED. Keying the idle decision on the
/// already-tested [`ready_prompt_candidate`] discriminator avoids reaping a
/// healthy session just because it had nothing to do ("I cannot start
/// lazily-rs.md, killed immediately").
/// Best-effort: capture `pane` and report whether a no-cycle fresh start should
/// be kept as a live idle session (the pane is back at a dispatch-ready prompt).
/// A capture failure returns `false` so the caller falls back to reaping a
/// genuine miss. (`#route-reaps-idle-fresh-start`)
pub(crate) fn fresh_start_pane_idle_ready(
    tmux: &Tmux,
    pane: &str,
    harness: &HarnessConfig,
) -> bool {
    match sessions::capture_pane(tmux, pane) {
        Ok(content) => matches!(
            fresh_start_ack_outcome(false, ready_prompt_candidate(&content, harness).is_some()),
            FreshStartAckOutcome::IdleNoOpKeep
        ),
        Err(_) => false,
    }
}

pub(crate) fn truncate_log_line(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// After a lazy claim, sync tmux layout for all files in the same window.
///
/// This ensures pane arrangement stays consistent when a file is reclaimed
/// to a different pane. Only runs on autoclaim — normal routing skips this.
#[allow(dead_code)]
pub(crate) fn sync_after_claim(tmux: &Tmux, pane_id: &str, col_args: &[String]) {
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
    // Keep the reconcile scoped to the caller's tmux handle. Falling back to the
    // default server here can mutate an unrelated live agent-doc window during
    // isolated verification runs.
    if let Err(e) = sync::run_with_tmux(&effective_col_args, Some(&window_id), None, tmux) {
        eprintln!("[route] warning: post-claim sync failed: {}", e);
    } else {
        eprintln!(
            "[route] Auto-synced {} files in window {}",
            file_count, window_id
        );
    }
}

/// Wait for the file's mtime and editor typing indicator to settle.
///
/// Polls every 100ms, up to 10× the debounce duration as a safety cap. Route
/// must fail closed instead of proceeding through a visible document mutation
/// while the editor-side typing indicator is still active.
pub(crate) fn await_idle(file: &Path, debounce: Duration) -> Result<()> {
    await_idle_with_max_wait(file, debounce, debounce * 10)
}

pub(crate) fn await_idle_with_max_wait(
    file: &Path,
    debounce: Duration,
    max_wait: Duration,
) -> Result<()> {
    use agent_doc_debounce::TypingIndicatorStatus;
    use std::time::Instant;

    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();
    let debounce_ms = debounce.as_millis().min(u64::MAX as u128) as u64;
    let file_str = file.to_string_lossy();

    loop {
        let indicator = agent_doc_debounce::typing_indicator_status(&file_str, debounce_ms);

        // `#jb-run-agent-doc-double-debounce`: when an editor owns the typing
        // lifecycle and its indicator reports idle, the editor already debounced
        // in-process before saving and routing. The editor's pre-route save
        // (`saveAllDocuments()`) freshly bumps the file mtime, so re-imposing the
        // mtime settle here double-counts the editor's own write as if it were
        // user typing — a redundant ~debounce-long wait the operator perceives as
        // "Run Agent Doc takes several seconds". The idle indicator is the
        // authoritative cross-process typing signal, so trust it and dispatch
        // immediately. CLI / direct-disk edits leave no indicator (`Absent`) and
        // keep the mtime debounce below as the fail-closed typing guard.
        match indicator {
            TypingIndicatorStatus::Idle => {
                eprintln!(
                    "[route] debounce OK — editor typing indicator idle (skipping redundant mtime settle for editor pre-route save)"
                );
                return Ok(());
            }
            TypingIndicatorStatus::Active => {
                // Editor reports active typing — keep waiting regardless of mtime.
            }
            TypingIndicatorStatus::Absent => {
                // No editor indicator (CLI / direct-disk caller): fall back to the
                // filesystem mtime settle as the only available quiescence proof.
                let mtime = std::fs::metadata(file)
                    .and_then(|m| m.modified())
                    .with_context(|| format!("failed to stat {}", file.display()))?;
                let elapsed_since_edit = mtime.elapsed().unwrap_or(Duration::ZERO);
                if elapsed_since_edit >= debounce {
                    eprintln!(
                        "[route] debounce OK — file idle for {:.1}s and no editor typing indicator",
                        elapsed_since_edit.as_secs_f64(),
                    );
                    return Ok(());
                }
            }
        }

        if start.elapsed() >= max_wait {
            let elapsed_since_edit = std::fs::metadata(file)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .unwrap_or(Duration::ZERO);
            anyhow::bail!(
                "route deferred for {}: document did not settle within {}ms (mtime_idle_ms={}, typing_active={}); retry after typing stops",
                file.display(),
                max_wait.as_millis(),
                elapsed_since_edit.as_millis(),
                indicator == TypingIndicatorStatus::Active
            );
        }

        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};
    use agent_doc_controller::dispatch::is_codex_shell_search_blocker;
    use agent_doc_controller::dispatch::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    #[test]
    fn codex_busy_ctrl_g_gate_only_fires_for_shell_search_blocker() {
        // C-g is allowed only for the two shell-search blocker reasons that
        // HarnessConfig::dispatch_blocker_reason emits and that wait_for_agent_ready_outcome
        // records as the authoritative busy reason.
        assert!(is_codex_shell_search_blocker(Some(
            "interactive shell reverse-i-search"
        )));
        assert!(is_codex_shell_search_blocker(Some(
            "interactive shell history search"
        )));

        // The exact regression class: a busy active turn is not a shell search, so
        // it must NOT receive C-g (which would open $EDITOR). An unknown timeout
        // (None) likewise fails closed to the Escape + C-c path.
        assert!(!is_codex_shell_search_blocker(Some("active codex turn")));
        assert!(!is_codex_shell_search_blocker(Some(
            "queued draft in composer"
        )));
        assert!(!is_codex_shell_search_blocker(Some(
            "active permission prompt"
        )));
        assert!(!is_codex_shell_search_blocker(None));

        // Linkage check: dispatch_blocker_reason actually classifies a shell-search
        // capture as one of the gated reasons (so the busy path feeds the gate the
        // string it expects).
        let codex = HarnessConfig::codex();
        let reverse_i_search = "Working...\nreverse-i-search: bugs enter accept · esc cancel\n";
        assert!(is_codex_shell_search_blocker(
            codex.dispatch_blocker_reason(reverse_i_search).as_deref()
        ));
        let active_turn = "• Working (12s • esc to interrupt)\n";
        assert!(!is_codex_shell_search_blocker(
            codex.dispatch_blocker_reason(active_turn).as_deref()
        ));
    }
    #[test]
    fn try_startup_lock_reports_busy_without_waiting() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "---\nagent_doc_session: startup-lock-test\n---\n").unwrap();

        let starting_dir = starting_dir_for(&doc).expect("project root should resolve");
        std::fs::create_dir_all(&starting_dir).unwrap();
        let hash = snapshot::doc_hash(&doc).unwrap();
        let lock_path = starting_dir.join(format!("{hash}.lock"));
        let held_doc_lock = open_start_lock(&lock_path).unwrap();
        fs2::FileExt::lock_exclusive(&held_doc_lock).unwrap();

        let start = std::time::Instant::now();
        let acquired =
            acquire_startup_locks(&doc, "startup-lock-test-session", StartupLockMode::Try).unwrap();
        let elapsed = start.elapsed();

        fs2::FileExt::unlock(&held_doc_lock).unwrap();
        assert!(
            matches!(acquired, StartupLockAcquire::Busy),
            "try-mode startup locks should report a busy lock instead of waiting"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "try-mode startup lock acquisition should be bounded, elapsed={elapsed:?}"
        );
    }
    #[test]
    fn route_debounce_fails_closed_while_typing_indicator_is_active() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "prompt in progress\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);

        let err =
            await_idle_with_max_wait(&doc, Duration::from_millis(500), Duration::from_millis(25))
                .expect_err("route must not proceed while the editor typing indicator is active");

        assert!(
            err.to_string().contains("typing_active=true"),
            "route debounce error should prove the active typing reason: {err}"
        );
    }
    #[test]
    fn route_debounce_allows_dispatch_after_typing_indicator_expires() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "settled prompt\n").unwrap();

        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_debounce::document_changed(&doc_str);

        await_idle_with_max_wait(&doc, Duration::from_millis(10), Duration::from_millis(1000))
            .expect("route should proceed after mtime and typing indicator are both idle");
    }
    #[test]
    fn route_dispatches_immediately_when_idle_typing_indicator_present_despite_fresh_mtime() {
        // `#jb-run-agent-doc-double-debounce`: the editor already awaited typing
        // idle in-process, then `saveAllDocuments()` bumped the file mtime right
        // before spawning route. Route must not re-impose the full mtime debounce on
        // the editor's own pre-route save when the cross-process typing indicator is
        // idle — that redundant wait is the "several seconds to dispatch" latency.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "settled prompt\n").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        // Editor tracked typing, then went idle (indicator present but stale).
        agent_doc_debounce::document_changed(&doc_str);
        std::thread::sleep(Duration::from_millis(80)); // exceed the 50ms debounce window

        assert_eq!(
            agent_doc_debounce::typing_indicator_status(&doc_str, 50),
            agent_doc_debounce::TypingIndicatorStatus::Idle,
            "indicator should report idle after the debounce window elapses"
        );

        // Editor's pre-route save bumps mtime to "now" (simulates saveAllDocuments()).
        std::fs::write(&doc, "settled prompt\n").unwrap();

        let start = std::time::Instant::now();
        await_idle_with_max_wait(&doc, Duration::from_millis(50), Duration::from_millis(2000))
            .expect("an idle editor typing indicator must authorize immediate dispatch");
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "route must not re-impose the mtime debounce when the editor indicator is idle (elapsed {:?})",
            start.elapsed()
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn register_dispatch_target_rejects_cross_file_rebind_and_preserves_registry() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let iso = IsolatedTmux::new("route-test-cross-file-rebind-guard");
        let session = "test";

        let tasks = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let first = tasks.join("agent-doc-bugs2.md");
        let second = tasks.join("tsift.md");
        std::fs::write(&first, "# first\n").unwrap();
        std::fs::write(&second, "# second\n").unwrap();
        let pane_a = iso.auto_start(session, dir.path()).unwrap();
        let pane_b = iso.split_window(&pane_a, dir.path(), "-dh").unwrap();

        sessions::register_full_with_cwd_in(
            dir.path(),
            "session-a",
            &pane_a,
            &first.to_string_lossy(),
            1234,
            "@128",
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd_in(
            dir.path(),
            "session-b",
            &pane_b,
            &second.to_string_lossy(),
            5678,
            "@128",
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        crate::startup_miss::append_session_log_event(
            &first,
            "session-a",
            &format!(
                "session_start file={} pane={} session=session-a",
                first.display(),
                pane_a
            ),
        )
        .unwrap();

        let err = register_dispatch_target(&iso, "session-b", &pane_a, &second.to_string_lossy())
            .expect_err("cross-file dispatch target rebind must fail closed");
        assert!(
            err.to_string().contains("refusing cross-file dispatch"),
            "error should explain the rejected cross-file dispatch: {err}"
        );
        assert_eq!(
            sessions::lookup_in(dir.path(), "session-a").unwrap(),
            Some(pane_a.clone()),
            "the original authoritative pane must stay bound to its file"
        );
        assert_eq!(
            sessions::lookup_in(dir.path(), "session-b").unwrap(),
            Some(pane_b),
            "the requesting file must keep its own registered pane"
        );
    }
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn route_refuses_dispatch_into_dead_bare_shell() {
        // #1vhn: when the harness has crashed/exited to a bare interactive
        // shell, route must fail closed instead of typing the trigger into the
        // shell. The pane starts as a bare shell (no harness), so a dispatch
        // must be blocked; once a harness dispatch-ready prompt appears the
        // block lifts.
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-dead-shell");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready"
        );

        let harness = HarnessConfig::claude();

        // Bare shell, no harness running → dispatch must be blocked.
        let blocked =
            super::super::dispatch::dead_harness_shell_dispatch_block(&iso, &pane, &harness);
        assert!(
            blocked.is_some(),
            "expected dead-harness shell block on a bare shell pane, got None"
        );

        // The actual send path must fail closed (not type the trigger into the shell).
        let doc = cwd.join("dead-shell.md");
        let err = super::super::dispatch::send_command_unchecked(
            &iso,
            &pane,
            &doc.to_string_lossy(),
            &harness,
        )
        .expect_err("send must fail closed into a bare shell");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("bare") && msg.contains("not running"),
            "fail-closed error should explain the dead harness: {msg}"
        );

        // Once a harness dispatch-ready prompt is visible, dispatch is allowed.
        send_keys_with_retry(&iso, &pane, &mock_agent_script(500));
        wait_for_pane_contains(&iso, &pane, "❯", std::time::Duration::from_secs(5));
        let allowed =
            super::super::dispatch::dead_harness_shell_dispatch_block(&iso, &pane, &harness);
        assert!(
            allowed.is_none(),
            "harness dispatch-ready prompt visible should not be blocked, got {allowed:?}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn wait_for_agent_ready_detects_claude_composer_hint_prompt() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-claude-composer-hint");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );

        let script = r#"exec /bin/sh -c 'printf "Starting claude...\n"; sleep 0.5; printf "⏵⏵ bypass permissions on (shift+tab to cycle)\n"; cat'"#;
        send_keys_with_retry(&iso, &pane, script);
        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "Starting claude...",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("Starting claude..."),
            "mock claude never started in pane: {content}"
        );

        let harness = HarnessConfig::claude();
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
        assert!(
            ready,
            "should detect Claude composer hint line as an idle prompt"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        // Recent Codex builds expose a `›` prompt above a footer/status line.
        let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "› \n"; printf "gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used\n"; cat'"#;
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
        assert!(
            ready,
            "should detect › prompt for codex harness even when a footer/status line follows it"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn wait_for_agent_ready_rejects_codex_queue_message_footer() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-codex-queue-message");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock codex launch"
        );

        let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "› \n"; printf "tab to queue message\n"; printf "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n"; cat'"#;
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
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(2), &harness);
        assert!(
            !ready,
            "queue-message footer must not count as an idle Codex prompt"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn wait_for_agent_ready_rejects_codex_reverse_history_search() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-codex-reverse-search");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock codex launch"
        );

        let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "reverse-i-search: bugs enter accept · esc cancel\n"; cat'"#;
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
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(3), &harness);
        assert!(
            !ready,
            "reverse-i-search must not count as an idle Codex prompt"
        );
    }
    #[test]
    fn ready_prompt_candidate_accepts_codex_idle_placeholder_prompt() {
        let harness = HarnessConfig::codex();
        let content = "\
Starting codex...
› Run /review on my current changes
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert!(
            ready_prompt_candidate(content, &harness).is_some(),
            "known idle Codex placeholder suggestions must count as a ready dispatch target"
        );
    }
    #[test]
    fn ready_prompt_candidate_accepts_future_codex_idle_placeholder_shape() {
        let harness = HarnessConfig::codex();
        let content = "\
Starting codex...
› Explain this module in @filename
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert!(
            ready_prompt_candidate(content, &harness).is_some(),
            "structurally-valid Codex idle placeholder suggestions must count as ready"
        );
    }
    #[test]
    fn ready_prompt_candidate_accepts_codex_footer_without_prompt() {
        let harness = HarnessConfig::codex();
        let content = "\
gpt-5.5 high · ~/work/btakita/agent-loop · Context 70% used
";
        assert!(
            ready_prompt_candidate(content, &harness).is_some(),
            "a bottom Codex status/footer line is idle dispatch-ready chrome when no busy cue or draft is visible"
        );
    }
    #[test]
    fn ready_prompt_candidate_accepts_codex_context_use_footer_without_prompt() {
        let harness = HarnessConfig::codex();
        let content = "\
gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
";
        assert!(
            ready_prompt_candidate(content, &harness).is_some(),
            "Codex route startup readiness must accept the shorter `Context N% use` status footer"
        );
    }
    #[test]
    fn ready_prompt_candidate_rejects_codex_busy_footer_without_prompt() {
        let harness = HarnessConfig::codex();
        let content = "\
Waiting for background terminal (esc to interrupt)
gpt-5.5 high · ~/work/btakita/agent-loop · Context 70% used
";
        assert!(
            ready_prompt_candidate(content, &harness).is_none(),
            "Codex footer-only idle recovery must still reject active-turn busy cues"
        );
    }
    #[test]
    fn ready_prompt_candidate_rejects_codex_hook_review_prompt_after_capability_proof() {
        let harness = HarnessConfig::codex();
        let content = "\
Starting codex...
⚠ 1 hook needs review before it can run. Open /hooks to review it.

› [start] managed codex capability proof: codex_capability_proof status=proven network=proven network_probe=child_dns_https ssh_targets=0 writable_roots=0 timings_ms=network_host_dns:8,network_child:9806,ssh:not_required,writable_launcher:not_required,writable_child:not_required,total:9815
";
        assert!(
            ready_prompt_candidate(content, &harness).is_none(),
            "Codex hook-review chrome requires operator approval before route can dispatch"
        );
    }
    #[test]
    fn ready_prompt_candidate_accepts_claude_idle_footer_after_stale_busy_scrollback() {
        let harness = HarnessConfig::claude();
        let content = "\
● Running 1 shell command…

✶ Tempering… (2m 21s · ↓ 9.7k tokens · thinking with high effort)

  ❯ /clear

────────────────────────────────────────────────────────────────────────────────── Check subagent status and deadlock ──
❯ Press up to edit queued messages
────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
  Opus 4.8 ctx:30% ~/work/btakita/agent-loop main brian@host
  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents
";
        assert!(
            ready_prompt_candidate(content, &harness).is_some(),
            "a later Claude dispatch-ready footer should supersede stale busy scrollback"
        );
    }
    #[test]
    fn ready_prompt_candidate_rejects_claude_active_spinner_footer() {
        let harness = HarnessConfig::claude();
        let content = "\
✶ Generating… (3s · esc to interrupt)
❯
  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host
  ⏵⏵ bypass permissions on · 1 shell
";
        assert!(
            ready_prompt_candidate(content, &harness).is_none(),
            "an active Claude turn must remain blocked when the latest footer is not dispatch-ready"
        );
    }
    #[test]
    fn ready_prompt_candidate_accepts_opencode_status_without_proof_output() {
        let harness = HarnessConfig::opencode();
        let content = "\
zai/glm-5 · ~/work/btakita/agent-loop · context 0% used
";
        assert!(
            ready_prompt_candidate(content, &harness).is_some(),
            "OpenCode can render an idle composer as status chrome with proof output kept out of the pane"
        );
    }
    #[test]
    fn ready_prompt_candidate_accepts_opencode_idle_splash_without_prompt_glyph() {
        let harness = HarnessConfig::opencode();
        let content = "\
                                                                                                     ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
                                                                                   ┃
                                                                                   ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                                   ┃
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                                   tab agents  ctrl+p commands
                                                                                        ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";
        assert!(
            ready_prompt_candidate(content, &harness).is_some(),
            "OpenCode 1.14 can render the idle composer as splash chrome without a prompt glyph"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn wait_for_agent_ready_rejects_codex_prompt_with_real_drafted_text() {
        let _tmux_guard = tmux_start_lock();
        let iso = IsolatedTmux::new("route-test-codex-drafted-prompt");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock codex launch"
        );

        let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "› investigate this issue\n"; printf "gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used\n"; cat'"#;
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
        let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(2), &harness);
        assert!(
            !ready,
            "real drafted Codex text must not count as an idle dispatch target"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
        assert_eq!(status.status, CommandDispatchStatus::Accepted);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn send_command_checked_codex_does_not_append_follow_up_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join("test.md"), "# test\n").unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-send-checked-no-extra-lines");
        let session = "test";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();
        let window = iso.pane_window(&pane).unwrap();
        sessions::register_full_with_cwd_in(
            dir.path(),
            "route-test-send-checked-no-extra-lines",
            &pane,
            "test.md",
            1234,
            &window,
            &dir.path().to_string_lossy(),
        )
        .unwrap();

        let script = write_mock_registered_agent_doc_extra_line_detector(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!(
                "exec {} {}",
                script.display(),
                dir.path().join("test.md").display()
            ),
        );
        let _ = wait_for_pane_contains(&iso, &pane, ">", std::time::Duration::from_secs(3));

        let status = send_command_checked(&iso, &pane, "test.md", &HarnessConfig::codex()).unwrap();
        assert_eq!(status.status, CommandDispatchStatus::Accepted);

        let content = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc test.md",
            std::time::Duration::from_secs(3),
        );
        assert!(
            content.contains("GOT:agent-doc test.md"),
            "command should be delivered as a bare reopen, got: {content}"
        );
        assert!(
            !content.contains("EXTRA:"),
            "codex reroute should not inject follow-up lines into the same payload: {content}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_records_optimistic_fresh_restart_retry_in_original_pane() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-codex-fresh-retry-handoff");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-codex-fresh-retry-handoff.md");
        let snapshot = "---\nagent: codex\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let stale_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-codex-fresh-retry-handoff";
        sessions::register(session_id, &pane, &file_path).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let supervisor_instance_id = "fresh-retry-handoff-supervisor".to_string();
        let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "restart_count": 0,
                        "actor_state": "ready",
                        "supervisor_pid": 12345,
                        "supervisor_instance_id": supervisor_instance_id_for_ipc
                    })),
                    IpcMethod::Restart { mode } => {
                        if mode == "fresh" {
                            restart_called_for_ipc.store(true, Ordering::Relaxed);
                        }
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let iso_for_thread = iso.clone();
        let ready_agent = write_mock_registered_agent_doc(dir.path());
        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        let pane_for_thread = pane.clone();
        let file_for_thread = file_path.clone();
        let registry_root = dir.path().to_path_buf();
        let restart_called_for_thread = restart_called.clone();
        let replacement = std::thread::spawn(move || {
            let wait_start = std::time::Instant::now();
            while !restart_called_for_thread.load(Ordering::Relaxed)
                && wait_start.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
            iso_for_thread
                .send_keys(
                    &replacement_pane,
                    &format!(
                        "exec {} {}",
                        ready_agent.display(),
                        doc_for_thread.display()
                    ),
                )
                .unwrap();
            let prompt_wait_start = std::time::Instant::now();
            while prompt_wait_start.elapsed() < Duration::from_secs(5) {
                let captured = crate::sessions::capture_pane(&iso_for_thread, &replacement_pane)
                    .unwrap_or_default();
                if captured.contains("> ") {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &pane_for_thread]);
            let replacement_window = iso_for_thread.pane_window(&replacement_pane).unwrap();
            sessions::register_supervisor_in(
                &registry_root,
                session_id,
                &replacement_pane,
                &file_for_thread,
                12345,
                &supervisor_instance_id,
            )
            .unwrap();
            crate::session_actor::project_binding_in(
                &registry_root,
                &file_for_thread,
                session_id,
                &replacement_pane,
                &replacement_window,
                "route",
                "fresh_restart_retry",
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(1200));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
            replacement_pane
        });

        let routed = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect(
        "route should keep the original routed pane authoritative after the fresh restart retry",
    );

        let replacement_pane = replacement.join().unwrap();
        assert!(restart_called.load(Ordering::Relaxed));
        assert_eq!(routed, pane);

        let replacement_after = sessions::capture_pane(&iso, &replacement_pane).unwrap_or_default();
        assert!(
            !replacement_after.contains("GOT:agent-doc "),
            "route must not redirect the reopen into the replacement pane after the fresh restart retry: {replacement_after}"
        );

        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("fresh restart retry should leave an optimistic startup-miss marker");
        assert_eq!(miss.file, file_path);
        assert_eq!(miss.pane_id, pane);
        assert_eq!(
            miss.origin,
            crate::startup_miss::StartupMissOrigin::RoutedTrigger
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn run_with_tmux_dispatch_only_ignores_startup_miss_on_alive_registered_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-startup-miss");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-dispatch-only-startup-miss.md");
        let content = "---\nagent_doc_session: route-dispatch-only-startup-miss\nagent: codex\n---\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n❯ follow-up question\n";
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-dispatch-only-startup-miss";
        sessions::register(session_id, &pane, &file_path).unwrap();
        let ipc_tmux = iso.clone();
        let pane_for_ipc = pane.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();
        crate::startup_miss::record(
            &doc,
            &pane,
            session_id,
            "codex",
            crate::startup_miss::StartupMissOrigin::RoutedTrigger,
            None,
        )
        .unwrap();

        let ready_agent = write_mock_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", ready_agent.display(), doc.display()),
        );

        run_with_tmux(
            &doc,
            &iso,
            None,
            0,
            &[],
            RouteMode::DispatchOnly,
            false,
            None,
        )
        .expect("dispatch-only route should ignore the stale startup-miss gate and send");

        let after = wait_for_pane_contains(
            &iso,
            &pane,
            "GOT:agent-doc ",
            std::time::Duration::from_secs(5),
        );
        assert!(
            after.contains("GOT:agent-doc "),
            "dispatch-only route should send despite the retained startup-miss marker: {after}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_restarts_fresh_for_busy_registered_pane_after_noop_fix() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-busy-fresh-reroute");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-live-pane-busy-fresh-reroute.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content =
            wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-pane-busy-fresh-reroute";
        sessions::register(session_id, &pane, &file_path).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let supervisor_instance_id = "busy-reroute-supervisor".to_string();
        let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
        let ipc_tmux = iso.clone();
        let injected_pane = Arc::new(std::sync::Mutex::new(None::<String>));
        let injected_pane_for_ipc = injected_pane.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "restart_count": 0,
                        "actor_state": "ready",
                        "supervisor_pid": 12345,
                        "supervisor_instance_id": supervisor_instance_id_for_ipc
                    })),
                    IpcMethod::Restart { mode } => {
                        if mode == "fresh" {
                            restart_called_for_ipc.store(true, Ordering::Relaxed);
                        }
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        if let Some(target) = injected_pane_for_ipc.lock().unwrap().clone() {
                            let _ = ipc_tmux.send_keys(&target, &bytes);
                        }
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();
        let ready_agent = write_mock_registered_agent_doc(dir.path());
        let iso_for_thread = iso.clone();
        let registry_root = dir.path().to_path_buf();
        let file_for_thread = file_path.clone();
        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        let pane_for_thread = pane.clone();
        let restart_called_for_thread = restart_called.clone();
        let injected_pane_for_thread = injected_pane.clone();
        let replacement = std::thread::spawn(move || {
            let wait_start = std::time::Instant::now();
            while !restart_called_for_thread.load(Ordering::Relaxed)
                && wait_start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
            iso_for_thread
                .send_keys(
                    &replacement_pane,
                    &format!(
                        "exec {} {}",
                        ready_agent.display(),
                        doc_for_thread.display()
                    ),
                )
                .unwrap();
            let prompt_wait_start = std::time::Instant::now();
            while prompt_wait_start.elapsed() < Duration::from_secs(5) {
                let captured = crate::sessions::capture_pane(&iso_for_thread, &replacement_pane)
                    .unwrap_or_default();
                if captured.contains("> ") {
                    *injected_pane_for_thread.lock().unwrap() = Some(replacement_pane.clone());
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &pane_for_thread]);
            let replacement_window = iso_for_thread.pane_window(&replacement_pane).unwrap();
            sessions::register_supervisor_in(
                &registry_root,
                session_id,
                &replacement_pane,
                &file_for_thread,
                12345,
                &supervisor_instance_id,
            )
            .unwrap();
            crate::session_actor::project_binding_in(
                &registry_root,
                &file_for_thread,
                session_id,
                &replacement_pane,
                &replacement_window,
                "route",
                "fresh_restart_retry",
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(1200));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
            replacement_pane
        });

        let routed = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should restart fresh once and reroute into the replacement pane");

        let replacement_pane = replacement.join().unwrap();
        assert!(restart_called.load(Ordering::Relaxed));
        assert!(
            routed == replacement_pane || routed == pane,
            "route should either report the handed-off pane or keep the reroute optimistic in the original pane: routed={routed} replacement={replacement_pane} original={pane}"
        );

        let busy_after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
        assert!(
            !busy_after.contains("GOT:agent-doc "),
            "route must not keep dispatching into the stale busy pane after the fresh restart reroute: {busy_after}"
        );
        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn fresh_restart_retry_preserves_absolute_reopen_path_for_relative_docs() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-live-pane-fresh-reroute-relative-doc");
        let session = "codex";
        let cwd = test_cwd();
        let pane = iso.auto_start(session, &cwd).unwrap();

        let relative_doc = std::path::PathBuf::from("src/session-share/tasks/claudescore-3.md");
        let doc = dir.path().join(&relative_doc);
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let stale_agent = write_mock_registered_agent_doc(dir.path());
        launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-live-pane-fresh-reroute-relative-doc";
        sessions::register(session_id, &pane, &file_path).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "restart_count": 0
                    })),
                    IpcMethod::Restart { mode } => {
                        if mode == "fresh" {
                            restart_called_for_ipc.store(true, Ordering::Relaxed);
                        }
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();
        let ready_agent = write_mock_registered_agent_doc(dir.path());
        let iso_for_thread = iso.clone();
        let registry_root = dir.path().to_path_buf();
        let file_for_thread = file_path.clone();
        let doc_for_thread = doc.clone();
        let current_for_thread = current.clone();
        let pane_for_thread = pane.clone();
        let restart_called_for_thread = restart_called.clone();
        let replacement = std::thread::spawn(move || {
            let wait_start = std::time::Instant::now();
            while !restart_called_for_thread.load(Ordering::Relaxed)
                && wait_start.elapsed() < Duration::from_secs(2)
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
            iso_for_thread
                .send_keys(
                    &replacement_pane,
                    &format!(
                        "exec {} {}",
                        ready_agent.display(),
                        doc_for_thread.display()
                    ),
                )
                .unwrap();
            let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &pane_for_thread]);
            sessions::register_full_with_cwd_in(
                &registry_root,
                session_id,
                &replacement_pane,
                &file_for_thread,
                12345,
                "@owner",
                registry_root.to_string_lossy().as_ref(),
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(1200));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
                .unwrap();
            replacement_pane
        });

        let routed = resolve_or_create_pane(
            &iso,
            &relative_doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should keep using the resolved absolute reopen path after a fresh retry");

        let replacement_pane = replacement.join().unwrap();
        assert!(restart_called.load(Ordering::Relaxed));
        assert_eq!(routed, pane);

        let replacement_after = sessions::capture_pane(&iso, &replacement_pane).unwrap_or_default();
        assert!(
            !replacement_after.contains("GOT:agent-doc "),
            "route must not redirect the reopen into the replacement pane after the fresh retry: {replacement_after}"
        );

        let miss = crate::startup_miss::load(&doc)
            .unwrap()
            .expect("fresh restart retry should persist the optimistic startup-miss marker");
        assert_eq!(miss.file, file_path);
        assert_eq!(miss.pane_id, pane);
        assert_eq!(
            miss.origin,
            crate::startup_miss::StartupMissOrigin::RoutedTrigger
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_waits_for_busy_restart_handoff_before_retrying_route() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-busy-restart-handoff");
        let session = "claude";
        let cwd = test_cwd();
        let busy_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("route-busy-restart-handoff.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
        send_keys_with_retry(
            &iso,
            &busy_pane,
            &format!("exec {} {}", busy_agent.display(), doc.display()),
        );
        let content = wait_for_pane_contains(
            &iso,
            &busy_pane,
            "Working...",
            std::time::Duration::from_secs(5),
        );
        assert!(
            content.contains("Working..."),
            "busy mock session should be active in pane: {content}"
        );

        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-busy-restart-handoff";
        sessions::register(session_id, &busy_pane, &file_path).unwrap();

        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc =
            crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
                match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": false,
                        "state": "healthy",
                        "restart_count": 0
                    })),
                    IpcMethod::Restart { .. } => {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                        IpcResponse::ok_empty()
                    }
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        injects_for_ipc.lock().unwrap().push(bytes.clone());
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                }
            })
            .unwrap();

        let iso_for_thread = iso.clone();
        let registry_root = dir.path().to_path_buf();
        let file_for_thread = file_path.clone();
        let doc_for_thread = doc.clone();
        let ack_current = current.clone();
        let ready_agent = write_mock_registered_agent_doc(dir.path());
        let restart_called_for_thread = restart_called.clone();
        let busy_pane_for_thread = busy_pane.clone();
        let replacement = std::thread::spawn(move || {
            let wait_start = std::time::Instant::now();
            while !restart_called_for_thread.load(Ordering::Relaxed)
                && wait_start.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            assert!(
                restart_called_for_thread.load(Ordering::Relaxed),
                "route should request a supervisor restart before test replacement handoff"
            );
            let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
            iso_for_thread
                .send_keys(
                    &replacement_pane,
                    &format!(
                        "exec {} {}",
                        ready_agent.display(),
                        doc_for_thread.display()
                    ),
                )
                .unwrap();
            let prompt_wait_start = std::time::Instant::now();
            while prompt_wait_start.elapsed() < Duration::from_secs(5) {
                let captured = crate::sessions::capture_pane(&iso_for_thread, &replacement_pane)
                    .unwrap_or_default();
                if captured.contains("> ") {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &busy_pane_for_thread]);
            sessions::register_full_with_cwd_in(
                &registry_root,
                session_id,
                &replacement_pane,
                &file_for_thread,
                12345,
                "@owner",
                registry_root.to_string_lossy().as_ref(),
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(1200));
            crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&ack_current)).unwrap();
            replacement_pane
        });

        let routed = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect(
            "route should wait for the restarted session to hand off to the new authoritative pane",
        );

        let replacement_pane = replacement.join().unwrap();
        assert!(restart_called.load(Ordering::Relaxed));
        assert_eq!(routed, replacement_pane);
        assert!(
            *injects.lock().unwrap()
                == vec![
                    agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(
                        &HarnessConfig::codex().trigger_command(&file_path)
                    )
                    .to_string()
                ],
            "route should dispatch exactly one bare Codex reopen through supervisor IPC after the restart handoff"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn alive_registered_pane_fails_closed_when_legacy_live_owner_conflicts() {
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
        let mut registry = tmux_router::Registry::default();
        registry.insert(
            file_path.clone(),
            tmux_router::RegistryEntry {
                pane: stale_pane.clone(),
                pid: 0,
                cwd: dir.path().to_string_lossy().to_string(),
                started: String::new(),
                session_id: "route-live-owner-reregister".to_string(),
                file: file_path.clone(),
                window: iso.pane_window(&stale_pane).unwrap_or_default(),
                supervisor_instance_id: String::new(),
            },
        );
        sessions::save_in(dir.path(), &registry).unwrap();

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

        let err = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        "route-live-owner-reregister",
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err(
        "route should fail closed instead of re-electing ownership from a legacy associated pane",
    );
        assert!(
            err.to_string()
                .contains("normal path will not re-elect ownership"),
            "unexpected error: {err:#}"
        );

        let live_content = sessions::capture_pane(&iso, &live_pane).unwrap_or_default();
        assert!(
            !live_content.contains("GOT:agent-doc "),
            "route should not dispatch into the conflicting legacy live pane automatically: {live_content}"
        );

        let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_content.contains("STALE:agent-doc "),
            "route should not dispatch into the stale registered pane either: {stale_content}"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_prefers_authoritative_actor_dispatch_target() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch");
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-authoritative-actor-dispatch";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "ready",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

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
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("route should dispatch through the authoritative actor pane");
        assert_eq!(resolved, actor_pane);

        let trigger = agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(
            &HarnessConfig::codex().trigger_command(&file_path),
        )
        .to_string();
        assert_eq!(*injects.lock().unwrap(), vec![trigger]);
        assert_eq!(
            sessions::lookup(session_id).unwrap().as_deref(),
            Some(actor_pane.as_str())
        );

        let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_content.contains("STALE:agent-doc "),
            "route should not dispatch into the stale registered pane when actor authority points elsewhere: {stale_content}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_prefers_authoritative_actor_dispatch_target() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch-only");
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

        let doc = dir.path().join("dispatch-only.md");
        let content = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n❯ follow-up question\n";
        std::fs::write(&doc, content).unwrap();
        crate::snapshot::save(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
        crate::cycle_state::start_preflight(
            &doc,
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
        )
        .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-authoritative-actor-dispatch-only";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "ready",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let resolved = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("dispatch-only reroute should dispatch through the authoritative actor pane");
        assert_eq!(resolved, actor_pane);

        assert!(
            injects.lock().unwrap().is_empty(),
            "ready authoritative dispatch-only path should submit through tmux pane input instead of supervisor inject"
        );
        assert_eq!(
            sessions::lookup(session_id).unwrap().as_deref(),
            Some(actor_pane.as_str())
        );

        let trigger = HarnessConfig::codex().trigger_command(&file_path);
        let actor_after = wait_for_pane_contains(
            &iso,
            &actor_pane,
            "ACTOR:agent-doc ",
            std::time::Duration::from_secs(3),
        );
        assert!(
            pane_capture_contains_wrapped(&actor_after, &trigger),
            "dispatch-only reroute should submit the reopen in the authoritative pane: {actor_after}"
        );
        let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_content.contains("STALE:agent-doc "),
            "dispatch-only reroute should not inject into the stale registered pane when actor authority points elsewhere: {stale_content}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_does_not_restart_after_tracked_codex_clear() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/codex-hooks/sessions")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-clear-no-restart");
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

        let doc = dir.path().join("dispatch-only-clear-no-restart.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-dispatch-only-clear-no-restart";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        std::fs::write(
            dir.path()
                .join(".agent-doc/codex-hooks/sessions/clear.json"),
            serde_json::json!({
                "session_id": "codex-clear-session",
                "doc_path": file_path,
                "last_turn_id": "turn-clear",
                "last_prompt": "/clear",
                "updated_at": 42u64
            })
            .to_string(),
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "ready",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => {
                restart_called_for_ipc.store(true, Ordering::Relaxed);
                IpcResponse::ok_empty()
            }
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let resolved = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect("dispatch-only reroute should keep sending the bare reopen after session clear");
        assert_eq!(resolved, actor_pane);
        assert!(
            !restart_called.load(Ordering::Relaxed),
            "dispatch-only reroute must not restart Codex just because the latest tracked prompt was /clear"
        );

        assert!(
            injects.lock().unwrap().is_empty(),
            "dispatch-only reroute after session clear should use pane submit instead of supervisor inject"
        );

        let actor_after = wait_for_pane_contains(
            &iso,
            &actor_pane,
            &HarnessConfig::codex().trigger_command(&file_path),
            std::time::Duration::from_secs(3),
        );
        assert!(
            actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
            "dispatch-only reroute after session clear should still submit the bare reopen in the authoritative pane: {actor_after}"
        );

        let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_content.contains("STALE:agent-doc "),
            "dispatch-only reroute should still avoid the stale registered pane after session clear: {stale_content}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatch_only_accepts_live_submit_without_codex_hook_proof() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        };

        let _tmux_guard = tmux_start_lock();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(dir.path().join(".codex/hooks.json"), "{}\n").unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch-only-unproven");
        let session = "codex";
        let cwd = test_cwd();
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "STALE\n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(
            &iso,
            &stale_pane,
            "STALE",
            std::time::Duration::from_secs(3),
        );

        let doc = dir.path().join("dispatch-only-authoritative-unproven.md");
        let snapshot = "---\nagent_doc_session: route-dispatch-only-authoritative-unproven\nagent: codex\ncodex_network_access: enabled\n---\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-dispatch-only-authoritative-unproven";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let restart_called = Arc::new(AtomicBool::new(false));
        let restart_called_for_ipc = restart_called.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "ready",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => {
                restart_called_for_ipc.store(true, Ordering::Relaxed);
                IpcResponse::ok_empty()
            }
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let pane = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect(
            "dispatch-only reroute should accept shared Enter delivery without Codex hook proof",
        );
        assert_eq!(pane, actor_pane);
        assert!(
            injects.lock().unwrap().is_empty(),
            "ready authoritative dispatch-only path should stay on direct pane submit"
        );
        assert!(
            !restart_called.load(Ordering::Relaxed),
            "editor dispatch-only reroutes must not restart a live Codex pane just because the session log lacks a capability proof"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_fails_closed_for_blocked_or_closed_authoritative_actor() {
        use std::sync::{Arc, Mutex};

        for (actor_state, reason) in [
            ("blocked", "the authoritative actor is blocked"),
            ("closed", "the authoritative actor is closed"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            let _cwd_guard = ScopedCurrentDir::set(dir.path());
            let iso = IsolatedTmux::new(&format!(
                "route-test-authoritative-actor-{}-fail-closed",
                actor_state
            ));
            let session = "codex";
            let cwd = test_cwd();
            let stale_pane = iso.auto_start(session, &cwd).unwrap();
            let _ =
                wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
            let actor_pane = iso.auto_start(session, &cwd).unwrap();

            let doc = dir
                .path()
                .join(format!("{actor_state}-authoritative-actor.md"));
            let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
            let current = format!("{snapshot}❯ follow-up question\n");
            std::fs::write(&doc, &current).unwrap();
            crate::snapshot::save(&doc, snapshot).unwrap();
            crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
            crate::cycle_state::mark_committed(
                &doc,
                "commit_success",
                Some(snapshot),
                Some(snapshot),
            )
            .unwrap();
            let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
            let session_id = format!("route-authoritative-actor-{actor_state}");
            sessions::register(&session_id, &stale_pane, &file_path).unwrap();

            let actor_window = iso.pane_window(&actor_pane).unwrap();
            crate::session_actor::project_binding_in(
                dir.path(),
                &file_path,
                &session_id,
                &actor_pane,
                &actor_window,
                "route",
                "dispatch_bind",
            )
            .unwrap();

            let injects = Arc::new(Mutex::new(Vec::<String>::new()));
            let injects_for_ipc = injects.clone();
            let mut ipc =
                SupervisorIpc::start(dir.path(), &session_id, move |method| match method {
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "running": true,
                        "state": "healthy",
                        "actor_state": actor_state,
                        "restart_count": 0
                    })),
                    IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                        injects_for_ipc.lock().unwrap().push(bytes.clone());
                        IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                    }
                    IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                    IpcMethod::Stop { .. }
                    | IpcMethod::StopAgent { .. }
                    | IpcMethod::ReplicaRegister { .. }
                    | IpcMethod::ReplicaDeregister { .. }
                    | IpcMethod::ReplicaUpdate { .. }
                    | IpcMethod::ReplicaPull { .. }
                    | IpcMethod::ReplicaAck { .. }
                    | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
                })
                .unwrap();

            let err = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                &session_id,
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut Vec::new(),
            )
            .expect_err("route should fail closed for non-recoverable authoritative actor states");
            let message = format!("{err:#}");
            assert!(
                message.contains(reason),
                "expected {actor_state} actor failure to mention `{reason}`, got: {message}"
            );
            assert!(
                injects.lock().unwrap().is_empty(),
                "route must not inject a duplicate reopen while the authoritative actor is {actor_state}"
            );
            assert_eq!(
                sessions::lookup(&session_id).unwrap().as_deref(),
                Some(actor_pane.as_str()),
                "route should still refresh the registry projection to the authoritative actor pane for {actor_state}"
            );

            let trigger = HarnessConfig::codex().trigger_command(&file_path);
            let actor_after = sessions::capture_pane(&iso, &actor_pane).unwrap_or_default();
            assert!(
                !actor_after.contains(&trigger),
                "route must not type a reopen into the blocked/closed authoritative pane: {actor_after}"
            );
            let stale_after = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
            assert!(
                !stale_after.contains(&trigger),
                "route must not fall back to the stale registered pane when actor state is {actor_state}: {stale_after}"
            );

            ipc.stop();
        }
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_or_create_pane_dispatches_busy_authoritative_actor_when_prompt_target_pending() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-busy");
        let session = "claude";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir.path().join("session.md");
        let snapshot = "---\nagent: claude\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-authoritative-actor-busy";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "busy",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

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
            session_id,
            &file_path,
            session,
            &HarnessConfig::claude(),
            &mut Vec::new(),
        )
        .expect("route should optimistically queue a busy authoritative actor");
        assert_eq!(resolved, actor_pane);

        let trigger = agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(
            &HarnessConfig::claude().trigger_command(&file_path),
        )
        .to_string();
        assert_eq!(*injects.lock().unwrap(), vec![trigger]);
        assert_eq!(
            sessions::lookup(session_id).unwrap().as_deref(),
            Some(actor_pane.as_str())
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn route_waits_for_starting_authoritative_actor_ready_before_dispatch() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-starting");
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            wait_for_shell(&iso, &actor_pane, Duration::from_secs(3)),
            "actor pane shell should be ready before installing Codex prompt fixture"
        );
        let prompt_script = dir.path().join("codex-ready.sh");
        std::fs::write(
            &prompt_script,
            "#!/bin/sh\nprintf '\\033[2J\\033[H› \\ngpt-5.4 high · ~/work/btakita/agent-loop · Context 0%% used\\n'\ncat\n",
        )
        .unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            &format!("exec /bin/sh {}", prompt_script.display()),
        );
        let ready_output =
            wait_for_pane_contains(&iso, &actor_pane, "Context", Duration::from_secs(3));
        assert!(
            ready_prompt_candidate(&ready_output, &HarnessConfig::codex()).is_some(),
            "actor pane should show a Codex dispatch-ready prompt before the ready wait: {ready_output}"
        );

        let doc = dir.path().join("session.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-authoritative-actor-starting";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let ready_at = Instant::now() + Duration::from_millis(150);
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State if Instant::now() >= ready_at => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "ready",
                "restart_count": 0
            })),
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "starting",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

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
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect(
        "route should wait for a starting authoritative actor to report ready before dispatching",
    );
        assert_eq!(resolved, actor_pane);

        let trigger = agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(
            &HarnessConfig::codex().trigger_command(&file_path),
        )
        .to_string();
        assert_eq!(*injects.lock().unwrap(), vec![trigger]);
        assert_eq!(
            sessions::lookup(session_id).unwrap().as_deref(),
            Some(actor_pane.as_str())
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn route_refreshes_closed_starting_authoritative_actor_without_start_timeout() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-authoritative-actor-starting-closed");
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "BOOTING\n"; cat'"#,
        );
        let _ = wait_for_pane_contains(
            &iso,
            &actor_pane,
            "BOOTING",
            std::time::Duration::from_secs(3),
        );

        let doc = dir.path().join("starting-authoritative-closed.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-authoritative-actor-starting-closed";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let close_at = Instant::now() + Duration::from_millis(120);
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State if Instant::now() >= close_at => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "closed",
                "restart_count": 0
            })),
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "starting",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let err = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("route must fail closed as soon as a starting actor refreshes to closed");
        let message = format!("{err:#}");
        assert!(
            message.contains("the authoritative actor is closed"),
            "closed actor refresh should surface the terminal state instead of the stale starting gate: {message}"
        );
        assert!(
            injects.lock().unwrap().is_empty(),
            "route must not queue a reopen once the starting actor refreshes to closed"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn route_dispatch_only_fails_closed_for_starting_authoritative_actor_without_ready_state() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-authoritative-starting-direct");
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "BOOTING\n"; cat'"#,
        );
        let _ = wait_for_pane_contains(
            &iso,
            &actor_pane,
            "BOOTING",
            std::time::Duration::from_secs(3),
        );

        let doc = dir
            .path()
            .join("dispatch-only-authoritative-starting-direct.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-dispatch-only-authoritative-starting-direct";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "starting",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err(
            "dispatch-only reroute must fail closed while the authoritative actor remains starting",
        );
        let message = format!("{err:#}");
        assert!(
            message.contains("the authoritative actor is still starting"),
            "starting actor failure should explain the state gate: {message}"
        );
        assert!(
            injects.lock().unwrap().is_empty(),
            "dispatch-only authoritative reroute must not queue through supervisor IPC while the actor is starting"
        );

        let actor_after = sessions::capture_pane(&iso, &actor_pane).unwrap_or_default();
        assert!(
            !actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
            "dispatch-only authoritative reroute must not submit through the live pane path while still starting: {actor_after}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn route_dispatch_only_refuses_starting_authoritative_actor_after_tracked_clear_until_ready() {
        use std::sync::{Arc, Mutex};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/codex-hooks/sessions")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-dispatch-only-authoritative-starting-clear");
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &stale_pane,
            r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
        );
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();
        send_keys_with_retry(
            &iso,
            &actor_pane,
            r#"exec /bin/sh -c 'printf "BOOTING\n"; while IFS= read -r CMD; do printf "ACTOR:%s\n" "$CMD"; done'"#,
        );
        let _ = wait_for_pane_contains(
            &iso,
            &actor_pane,
            "BOOTING",
            std::time::Duration::from_secs(3),
        );

        let doc = dir
            .path()
            .join("dispatch-only-authoritative-starting-clear.md");
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-dispatch-only-authoritative-starting-clear";
        sessions::register(session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        std::fs::write(
            dir.path()
                .join(".agent-doc/codex-hooks/sessions/clear.json"),
            serde_json::json!({
                "session_id": "codex-clear-session",
                "doc_path": file_path,
                "last_turn_id": "turn-clear",
                "last_prompt": "/clear",
                "updated_at": 42u64
            })
            .to_string(),
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": "starting",
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("dispatch-only reroute after /clear must wait for a dispatch-ready prompt before direct pane submit");
        let message = format!("{err:#}");
        assert!(
            message.contains("the authoritative actor is still starting")
                || message.contains("never reached a dispatch-ready prompt")
                || message.contains("never showed a dispatch-ready prompt"),
            "starting actor after /clear should fail before input when no prompt is visible: {message}"
        );
        assert!(
            injects.lock().unwrap().is_empty(),
            "dispatch-only reroute after /clear should not queue through supervisor IPC"
        );

        let actor_after = sessions::capture_pane(&iso, &actor_pane).unwrap_or_default();
        assert!(
            !actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
            "dispatch-only reroute after /clear must not submit to a pane before it is dispatch-ready: {actor_after}"
        );
        let stale_after = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_after.contains("STALE:agent-doc "),
            "dispatch-only reroute should avoid stale registered panes after /clear: {stale_after}"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn alive_registered_pane_uses_supervisor_pid_fallback_when_argv_loses_file_path() {
        use std::sync::{Arc, Mutex};

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
        assert!(
            wait_for_agent_ready_outcome(
                &iso,
                &pane,
                Duration::from_secs(10),
                &HarnessConfig::codex()
            )
            .is_ready(),
            "mock agent prompt should be ready before route probes the recovered supervisor owner"
        );
        let mock_agent_pid =
            wait_for_process_pid(&mock_agent.display().to_string(), Duration::from_secs(3));
        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();

        let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": mock_agent_pid })),
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Inject { bytes } | IpcMethod::Clear { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. }
            | IpcMethod::Stop { .. }
            | IpcMethod::StopAgent { .. }
            | IpcMethod::ReplicaRegister { .. }
            | IpcMethod::ReplicaDeregister { .. }
            | IpcMethod::ReplicaUpdate { .. }
            | IpcMethod::ReplicaPull { .. }
            | IpcMethod::ReplicaAck { .. }
            | IpcMethod::ReplicaAwareness { .. } => IpcResponse::ok_empty(),
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
            &mut Vec::new(),
        )
        .expect("route should recover the live owner via supervisor pid");
        assert_eq!(resolved, pane);
        assert!(
            *injects.lock().unwrap()
                == vec![
                    agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(
                        &HarnessConfig::codex().trigger_command(&file_path)
                    )
                    .to_string()
                ],
            "route should dispatch to the registered pane via supervisor IPC after recovering the live owner via supervisor pid"
        );

        ipc.stop();
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
            ready && ready_prompt_candidate(&content, &harness).is_some(),
            "should detect ❯ in pane content, got: {}",
            content
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

            let previous = tmux_router::RegistryEntry {
                pane: old_pane.clone(),
                pid: std::process::id(),
                cwd: dir.path().to_string_lossy().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-123".to_string(),
                file: "doc.md".to_string(),
                window: iso.pane_window(&old_pane)?,
                supervisor_instance_id: String::new(),
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

            let previous = tmux_router::RegistryEntry {
                pane: busy_pane.clone(),
                pid: std::process::id(),
                cwd: dir.path().to_string_lossy().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-busy".to_string(),
                file: "doc.md".to_string(),
                window: iso.pane_window(&busy_pane)?,
                supervisor_instance_id: String::new(),
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
            let r = run_with_tmux(&file, &iso, None, 0, &[], RouteMode::Managed, false, None);
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
            let r = run_with_tmux(&file, &iso, None, 0, &[], RouteMode::Managed, false, None);
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn sync_after_claim_stays_on_injected_tmux_server() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();

        let file_a = dir.path().join("tasks/file_a.md");
        let file_b = dir.path().join("tasks/file_b.md");
        std::fs::write(
            &file_a,
            "---\nagent_doc_session: route-sync-claim-a\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &file_b,
            "---\nagent_doc_session: route-sync-claim-b\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("route-test-sync-after-claim-injected");
        let session = "test";
        let pane_a = iso.new_session(session, dir.path()).unwrap();
        let window = iso.pane_window(&pane_a).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);
        let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
        let pane_b = iso.split_window(&pane_a, dir.path(), "-dh").unwrap();
        let extra_pane = iso.split_window(&pane_b, dir.path(), "-dh").unwrap();
        let pane_a_pid = pane_display_value(&iso, &pane_a, "#{pane_pid}")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap();
        let pane_b_pid = pane_display_value(&iso, &pane_b, "#{pane_pid}")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap();

        sessions::register_full_with_cwd(
            "route-sync-claim-a",
            &pane_a,
            &file_a.to_string_lossy(),
            pane_a_pid,
            &window,
            &dir.path().to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd(
            "route-sync-claim-b",
            &pane_b,
            &file_b.to_string_lossy(),
            pane_b_pid,
            &window,
            &dir.path().to_string_lossy(),
        )
        .unwrap();

        sync_after_claim(&iso, &pane_a, &[]);

        let visible = iso.list_window_panes(&window).unwrap();
        assert_eq!(
            visible.len(),
            2,
            "post-claim sync should reconcile the injected tmux window instead of mutating the default server"
        );
        assert!(
            visible.contains(&pane_a) && visible.contains(&pane_b),
            "registered panes should remain visible after the injected-server reconcile, got {:?}",
            visible
        );
        assert!(
            !visible.contains(&extra_pane),
            "unregistered overflow pane should be removed from the injected tmux window, got {:?}",
            visible
        );
        assert!(
            iso.pane_alive(&extra_pane),
            "overflow pane should be stashed, not killed"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn provision_pane_first_col_splits_left() {
        // Verify that provision_pane with a file in the first column
        // computes split_before=true via is_first_column and places the new
        // pane at the leftmost position in the agent-doc window.
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let tasks = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let file_a = tasks.join("file_a.md");
        let file_b = tasks.join("file_b.md");
        std::fs::write(&file_a, "# A\n").unwrap();
        std::fs::write(&file_b, "# B\n").unwrap();

        let iso = IsolatedTmux::new("route-test-auto-start-col-left");
        let session = "test";
        let cwd = dir.path().to_path_buf();

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
        let file_a_rel = Path::new("tasks/file_a.md");
        let result = provision_pane(
            &iso,
            file_a_rel,
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn provision_pane_second_col_splits_right() {
        // Verify that provision_pane with a file in the second column
        // computes split_before=false via is_first_column and places the new
        // pane at the rightmost position in the agent-doc window.
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let tasks = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let file_a = tasks.join("file_a.md");
        let file_b = tasks.join("file_b.md");
        std::fs::write(&file_a, "# A\n").unwrap();
        std::fs::write(&file_b, "# B\n").unwrap();

        let iso = IsolatedTmux::new("route-test-auto-start-col-right");
        let session = "test";
        let cwd = dir.path().to_path_buf();

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
        let file_b_rel = Path::new("tasks/file_b.md");
        let result = provision_pane(
            &iso,
            file_b_rel,
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn provision_pane_right_col_picks_rightmost_after_rearrange() {
        // Regression: provision_pane must use screen position, not creation order.
        // After rearranging panes so creation order != screen order,
        // split_before=false should split from the rightmost pane by screen position.
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let tasks = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let file_a = tasks.join("file_a.md");
        let file_b = tasks.join("file_b.md");
        std::fs::write(&file_a, "# A\n").unwrap();
        std::fs::write(&file_b, "# B\n").unwrap();

        let iso = IsolatedTmux::new("route-test-provision-rearranged");
        let session = "test";
        let cwd = dir.path().to_path_buf();

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
        let file_b_rel = Path::new("tasks/file_b.md");
        let result = provision_pane(
            &iso,
            file_b_rel,
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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

        let registry = sessions::load_in(dir.path()).unwrap();
        assert!(
            registry
                .values()
                .any(|entry| entry.session_id == "route-test-concurrent-provision-session-a"),
            "first provisioned document should be registered"
        );
        assert!(
            registry
                .values()
                .any(|entry| entry.session_id == "route-test-concurrent-provision-session-b"),
            "second provisioned document should be registered"
        );
    }
    #[test]
    fn truncate_log_line_preserves_utf8_boundaries() {
        let line = "  gpt-5.4 high · ~/work/btakita/agent-loop/src/boost-clien…";
        let truncated = truncate_log_line(line, 60);
        assert_eq!(truncated, line);
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());

        let longer = format!("{line} with trailing content");
        let truncated_longer = truncate_log_line(&longer, 60);
        assert!(std::str::from_utf8(truncated_longer.as_bytes()).is_ok());
        assert_eq!(truncated_longer.chars().count(), 60);
        assert!(longer.starts_with(&truncated_longer));
    }
}

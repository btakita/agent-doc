//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

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
    let base = snapshot::find_project_root(&canonical)
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

pub(crate) fn lock_startup_file(lock: &File, lock_path: &Path, mode: StartupLockMode) -> Result<bool> {
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
                    format_duplicate_pane_policy_error(
                        session_name,
                        file_path,
                        Some(target),
                        &format!("split-window failed alongside pane {} ({})", target, e)
                    )
                );
            }
        }
    } else {
        let has_agent_doc_window = has_named_window(tmux, session_name, "agent-doc");
        if has_agent_doc_window {
            anyhow::bail!(
                "{}",
                format_duplicate_pane_policy_error(
                    session_name,
                    file_path,
                    None,
                    "the target session already has an 'agent-doc' window but no safe registered anchor pane was found"
                )
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
        let ready =
            wait_for_agent_ready(tmux, &new_pane, std::time::Duration::from_secs(30), harness);
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

        let ack_timeout = fresh_route_start_ack_timeout();
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
                            crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
                            crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
                            crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
                            crate::cycle_state::CyclePhase::Committed => "committed",
                            crate::cycle_state::CyclePhase::Abandoned => "abandoned",
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
                    .map(prompt::strip_ansi)
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
    // OpenCode treats a bottom idle composer (including a bare status/footer
    // splash) as dispatch-ready. Codex does not: its composer always renders an
    // actual `›` dispatch-ready prompt when ready, so a status/footer line alone
    // is not a dispatch-ready prompt and must not be accepted as one.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshStartAckOutcome {
    CycleAcknowledged,
    IdleNoOpKeep,
    GenuineMissReap,
}

pub(crate) fn fresh_start_ack_outcome(
    cycle_acknowledged: bool,
    pane_capture: &str,
    harness: &HarnessConfig,
) -> FreshStartAckOutcome {
    if cycle_acknowledged {
        FreshStartAckOutcome::CycleAcknowledged
    } else if ready_prompt_candidate(pane_capture, harness).is_some() {
        FreshStartAckOutcome::IdleNoOpKeep
    } else {
        FreshStartAckOutcome::GenuineMissReap
    }
}

/// Best-effort: capture `pane` and report whether a no-cycle fresh start should
/// be kept as a live idle session (the pane is back at a dispatch-ready prompt).
/// A capture failure returns `false` so the caller falls back to reaping a
/// genuine miss. (`#route-reaps-idle-fresh-start`)
pub(crate) fn fresh_start_pane_idle_ready(tmux: &Tmux, pane: &str, harness: &HarnessConfig) -> bool {
    match sessions::capture_pane(tmux, pane) {
        Ok(content) => matches!(
            fresh_start_ack_outcome(false, &content, harness),
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

pub(crate) fn await_idle_with_max_wait(file: &Path, debounce: Duration, max_wait: Duration) -> Result<()> {
    use crate::debounce::TypingIndicatorStatus;
    use std::time::Instant;

    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();
    let debounce_ms = debounce.as_millis().min(u64::MAX as u128) as u64;
    let file_str = file.to_string_lossy();

    loop {
        let indicator = crate::debounce::typing_indicator_status(&file_str, debounce_ms);

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

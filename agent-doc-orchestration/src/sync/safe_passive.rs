//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn safe_passive_focus_path_and_session(
    focus: Option<&str>,
) -> Option<(PathBuf, String)> {
    let focus = focus?.trim();
    if focus.is_empty() {
        return None;
    }
    let focus_path = PathBuf::from(focus);
    let session_id = agent_doc_frontmatter_io::session::read_session_id(&focus_path)?;
    Some((focus_path, session_id))
}

pub(crate) fn safe_passive_local_actor_record_state(
    focus_path: &Path,
) -> Option<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    let canonical = focus_path
        .canonicalize()
        .ok()
        .unwrap_or_else(|| focus_path.to_path_buf());
    let base_dir = agent_doc_fs::find_project_root(&canonical)?;
    crate::session_actor::load_record_in(&base_dir, &canonical.to_string_lossy()).ok()
}

pub(crate) fn safe_passive_registry_pane_state(
    focus_path: &Path,
    session_id: &str,
) -> Option<Option<String>> {
    let canonical = focus_path
        .canonicalize()
        .ok()
        .unwrap_or_else(|| focus_path.to_path_buf());
    let base_dir = agent_doc_fs::find_project_root(&canonical)?;
    crate::sessions::lookup_in(&base_dir, session_id).ok()
}

/// Move-before-select for the passive fast-handoff focus path
/// (`#tmux-switch-lag`). If the actor pane is parked in a `stash` window,
/// reparent it into the working `agent-doc` window *before* selecting it, so the
/// doc-to-doc switch never shows an intermediate stash frame (the visible flash
/// the operator sees mid-switch). tmux preserves the pane id across the
/// `join-pane`/`break-pane` move, so the caller keeps selecting the same id.
/// Best-effort: a non-stashed pane or a failed promote leaves the later
/// `select_pane` to surface it in place, exactly as before.
pub(crate) fn promote_stashed_pane_before_focus(tmux: &Tmux, focus_path: &Path, pane_id: &str) {
    if !pane_in_stash_window(tmux, pane_id) {
        return;
    }
    match promote_pane_to_agent_doc_window(tmux, pane_id) {
        Ok(true) => sync_log(&format!(
            "safe_passive_move_before_select_promoted file={} pane={} (#tmux-switch-lag)",
            focus_path.display(),
            pane_id
        )),
        Ok(false) => sync_log(&format!(
            "safe_passive_move_before_select_noop file={} pane={} (#tmux-switch-lag)",
            focus_path.display(),
            pane_id
        )),
        Err(err) => {
            eprintln!(
                "[sync] warning: move-before-select promote of pane {} for {} failed: {}",
                pane_id,
                focus_path.display(),
                err
            );
            sync_log(&format!(
                "warning: safe_passive_move_before_select_failed file={} pane={} err={}",
                focus_path.display(),
                pane_id,
                err
            ));
        }
    }
}

pub(crate) fn safe_passive_select_prelock_pane(
    tmux: &Tmux,
    focus_path: &Path,
    pane_id: &str,
    source: &str,
) -> Option<String> {
    // Move-before-select: surface the pane out of stash before selecting it so
    // the switch shows no intermediate stash frame (#tmux-switch-lag).
    promote_stashed_pane_before_focus(tmux, focus_path, pane_id);
    if let Err(err) = tmux.select_pane(pane_id) {
        eprintln!(
            "[sync] warning: failed safe-passive pre-lock focus of actor pane {} for {}: {}",
            pane_id,
            focus_path.display(),
            err
        );
        sync_log(&format!(
            "warning: safe_passive_prelock_actor_focus_failed file={} pane={} source={} err={}",
            focus_path.display(),
            pane_id,
            source,
            err
        ));
        return None;
    }
    eprintln!(
        "[sync] safe_passive_prelock_actor_focus pane={} file={} source={}",
        pane_id,
        focus_path.display(),
        source
    );
    sync_log(&format!(
        "safe_passive_prelock_actor_focus file={} pane={} source={}",
        focus_path.display(),
        pane_id,
        source
    ));
    Some(pane_id.to_string())
}

pub(crate) fn safe_passive_prelock_provision_focus_pane(
    tmux: &Tmux,
    focus_path: &Path,
    session_id: &str,
    window: Option<&str>,
    col_args: &[String],
) -> Option<String> {
    if has_rename_debounce(focus_path) {
        let message = format!(
            "safe_passive_prelock_autostart_skipped file={} reason=rename_debounce",
            focus_path.display()
        );
        eprintln!("[sync] {}", message);
        sync_log(&message);
        return None;
    }

    match skip_auto_start_for_recent_session_loss(focus_path, session_id) {
        Ok(true) => return None,
        Ok(false) => {}
        Err(err) => {
            let message = format!(
                "warning: safe_passive_prelock_autostart_recent_loss_check_failed file={} err={}",
                focus_path.display(),
                err
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            return None;
        }
    }

    match passive_autostart_skip_reason(tmux, focus_path, session_id, None) {
        Ok(Some(reason)) => {
            let message = format!(
                "safe_passive_prelock_autostart_skipped file={} reason={}",
                focus_path.display(),
                reason
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            return None;
        }
        Ok(None) => {}
        Err(err) => {
            let message = format!(
                "warning: safe_passive_prelock_autostart_skip_check_failed file={} err={}",
                focus_path.display(),
                err
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            return None;
        }
    }

    let context_session = window.and_then(|target| session_name_for_target_window(tmux, target));
    let file_str = focus_path.to_string_lossy().to_string();
    match route::try_provision_pane(
        tmux,
        focus_path,
        session_id,
        &file_str,
        context_session.as_deref(),
        col_args,
    ) {
        Ok(Some(pane_id)) => {
            eprintln!(
                "[sync] safe_passive_prelock_autostart pane={} file={}",
                pane_id,
                focus_path.display()
            );
            sync_log(&format!(
                "safe_passive_prelock_autostart file={} pane={}",
                focus_path.display(),
                pane_id
            ));
            Some(pane_id)
        }
        Ok(None) => {
            sync_log(&format!(
                "safe_passive_prelock_autostart_skipped file={} reason=startup_lock_busy",
                focus_path.display()
            ));
            None
        }
        Err(err) => {
            let message = format!(
                "warning: safe_passive_prelock_autostart_failed file={} err={}",
                focus_path.display(),
                err
            );
            eprintln!("[sync] {}", message);
            sync_log(&message);
            None
        }
    }
}

pub(crate) fn safe_passive_focus_actor_before_sync_lock(
    tmux: &Tmux,
    focus: Option<&str>,
    window: Option<&str>,
    col_args: &[String],
) -> Option<String> {
    let (focus_path, session_id) = safe_passive_focus_path_and_session(focus)?;
    if let Some(pane_id) =
        crate::focus::local_actor_projection_pane_for_document(&focus_path, &session_id, tmux)
    {
        return safe_passive_select_prelock_pane(tmux, &focus_path, &pane_id, "local_projection");
    }

    match safe_passive_local_actor_record_state(&focus_path)? {
        Some(record) => {
            sync_log(&format!(
                "safe_passive_prelock_actor_focus_deferred file={} reason=local_actor_record_not_live record_session={} record_pane={} record_state={:?}",
                focus_path.display(),
                record.session_id,
                record.pane_id,
                record.state
            ));
            None
        }
        None => match safe_passive_registry_pane_state(&focus_path, &session_id)? {
            Some(pane_id) if tmux.pane_alive(&pane_id) => {
                safe_passive_select_prelock_pane(tmux, &focus_path, &pane_id, "sessions_registry")
            }
            Some(pane_id) => {
                sync_log(&format!(
                    "safe_passive_prelock_actor_focus_deferred file={} reason=registry_pane_not_live registry_pane={}",
                    focus_path.display(),
                    pane_id
                ));
                None
            }
            None => safe_passive_prelock_provision_focus_pane(
                tmux,
                &focus_path,
                &session_id,
                window,
                col_args,
            ),
        },
    }
}

pub(crate) fn safe_passive_focus_actor_after_sync_lock(
    tmux: &Tmux,
    focus: Option<&str>,
    proof_cache: &SyncProofCache,
) -> Option<String> {
    let (focus_path, session_id) = safe_passive_focus_path_and_session(focus)?;
    let (pane_id, generation, source) = if let Some(pane_id) =
        crate::focus::local_actor_projection_pane_for_document(&focus_path, &session_id, tmux)
    {
        (pane_id, None, "local_projection")
    } else {
        let record = load_live_authoritative_actor_record_cached(
            tmux,
            &focus_path,
            &session_id,
            proof_cache,
        )?;
        (record.pane_id, Some(record.generation), "controller")
    };
    // Move-before-select: surface the pane out of stash before selecting it so
    // the switch shows no intermediate stash frame (#tmux-switch-lag).
    promote_stashed_pane_before_focus(tmux, &focus_path, &pane_id);
    if let Err(err) = tmux.select_pane(&pane_id) {
        eprintln!(
            "[sync] warning: failed safe-passive post-lock focus of actor pane {} for {}: {}",
            pane_id,
            focus_path.display(),
            err
        );
        sync_log(&format!(
            "warning: safe_passive_postlock_actor_focus_failed file={} pane={} source={} err={}",
            focus_path.display(),
            pane_id,
            source,
            err
        ));
        return None;
    }
    let generation = generation
        .map(|generation| generation.to_string())
        .unwrap_or_else(|| "projection".to_string());
    eprintln!(
        "[sync] safe_passive_postlock_actor_focus pane={} file={} generation={} source={}",
        pane_id,
        focus_path.display(),
        generation,
        source
    );
    sync_log(&format!(
        "safe_passive_postlock_actor_focus file={} pane={} generation={} source={}",
        focus_path.display(),
        pane_id,
        generation,
        source
    ));
    Some(pane_id)
}

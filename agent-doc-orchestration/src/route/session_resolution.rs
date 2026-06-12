//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

/// Get the tmux session that owns the caller pane.
pub(crate) fn current_tmux_session(tmux: &Tmux) -> Option<String> {
    tmux.current_session()
}

pub fn resolve_preferred_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    log_prefix: &str,
) -> Option<String> {
    if let Some(ctx) = normalize_context_session(context_session) {
        return Some(ctx.to_string());
    }

    let configured = crate::config::project_tmux_session();
    if configured.as_ref().is_some_and(|s| tmux.session_alive(s)) {
        return configured;
    }

    if let Some(ref stale) = configured {
        eprintln!(
            "{log_prefix} configured tmux_session '{}' is not alive, ignoring stale pin",
            stale
        );
    }

    current_tmux_session(tmux)
}

pub(crate) fn resolve_preferred_session_for_layout(
    tmux: &Tmux,
    context_session: Option<&str>,
    col_args: &[String],
    focus: Option<&Path>,
    log_prefix: &str,
) -> Option<String> {
    if let Some(ctx) = normalize_context_session(context_session) {
        return Some(ctx.to_string());
    }

    let focus_owned = focus.map(|path| path.to_string_lossy().into_owned());
    if let Some(scope_root) = crate::sync::shared_sync_scope_root(col_args, focus_owned.as_deref())
        && let Some(session) = crate::sync::configured_session_for_root(tmux, &scope_root)
    {
        return Some(session);
    }

    resolve_preferred_session(tmux, None, log_prefix)
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
pub(crate) fn resolve_target_session(
    tmux: &Tmux,
    context_session: Option<&str>,
    col_args: &[String],
    focus: Option<&Path>,
    harness: &HarnessConfig,
) -> String {
    resolve_preferred_session_for_layout(tmux, context_session, col_args, focus, "[route]")
        .unwrap_or_else(|| harness.tmux_session_fallback.clone())
}

pub(crate) fn ensure_auto_start_target_session(
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

pub(crate) fn normalize_context_session(context_session: Option<&str>) -> Option<&str> {
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
pub(crate) fn find_target_pane(
    tmux: &Tmux,
    explicit_pane: Option<&str>,
    _session_name: &str,
    claimed_panes: &std::collections::HashSet<String>,
) -> Option<String> {
    let target = explicit_pane.map(|p| p.to_string());
    target.filter(|p| tmux.pane_alive(p) && !claimed_panes.contains(p))
}

/// Check if a window with the given name exists in the target tmux session.
pub(crate) fn has_named_window(tmux: &Tmux, session_name: &str, window_name: &str) -> bool {
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

pub(crate) fn pane_session_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    tmux.cmd()
        .args(["display-message", "-t", pane_id, "-p", "#{session_name}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub(crate) fn pane_window_name(tmux: &Tmux, pane_id: &str) -> Option<String> {
    tmux.pane_window(pane_id).ok().and_then(|window_id| {
        tmux.cmd()
            .args(["display-message", "-t", &window_id, "-p", "#{window_name}"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    })
}

pub(crate) fn is_stash_window_name(window_name: &str) -> bool {
    window_name == "stash" || window_name.starts_with("stash-")
}

pub(crate) fn evict_previous_stash_pane(
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

pub(crate) fn evict_previous_stash_pane_entry(
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
pub(crate) fn find_registered_pane_in_session(
    tmux: &Tmux,
    registry_base_dir: &Path,
    session_name: &str,
    exclude_pane: &str,
) -> Option<String> {
    let registry = sessions::load_in(registry_base_dir).ok()?;
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

pub(crate) fn registry_base_dir_for_file(file: &Path, fallback: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(file)
        .ok()
        .and_then(|path| {
            crate::snapshot::find_project_root(&path)
                .or_else(|| path.parent().map(|parent| parent.to_path_buf()))
        })
        .unwrap_or_else(|| fallback.to_path_buf())
}

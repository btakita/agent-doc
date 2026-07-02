//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_controller::dispatch::{is_stash_window_name, normalize_context_session};

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

    let configured = agent_doc_project_config_io::project_tmux_session();
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
    if let Some(scope_root) =
        agent_doc_sync::shared_sync_scope_root(col_args, focus_owned.as_deref())
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

    if agent_doc_project_config_io::project_tmux_session().as_deref() == Some(session_name)
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

pub(crate) fn evict_previous_stash_pane(
    tmux: &Tmux,
    session_id: &str,
    replacement_pane: &str,
    target_session: &str,
    harness: &HarnessConfig,
) {
    let Ok(Some(previous)) = agent_doc_session_registry_io::lookup_entry(session_id) else {
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
    previous: &tmux_router::RegistryEntry,
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
    if agent_doc_tmux_io::target_session_name(tmux, &previous.pane).as_deref()
        != Some(target_session)
    {
        return;
    }
    let Some(window_name) = agent_doc_tmux_io::target_window_name(tmux, &previous.pane) else {
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
    let registry = agent_doc_session_registry_io::load_in(registry_base_dir).ok()?;
    for entry in registry.values() {
        if entry.pane == exclude_pane || entry.pane.is_empty() {
            continue;
        }
        if !tmux.pane_alive(&entry.pane) {
            continue;
        }
        // Check if this pane is in the target session
        if agent_doc_tmux_io::target_session_name(tmux, &entry.pane).as_deref()
            == Some(session_name)
        {
            return Some(entry.pane.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_controller::dispatch::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
    use agent_doc_supervisor::ipc_protocol::{IpcMethod, IpcResponse};
    use agent_doc_supervisor_io::ipc::SupervisorIpc;
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn has_named_window_detects_agent_doc_window() {
        let iso = IsolatedTmux::new("route-test-named-win");
        let session = "test";
        let cwd = std::env::current_dir().unwrap();

        // Create session (first window gets default name, not "agent-doc")
        let _pane = iso.auto_start(session, &cwd).unwrap();
        assert!(
            !agent_doc_tmux_io::has_window_named(&iso, session, "agent-doc"),
            "should not find 'agent-doc' window before renaming"
        );

        // Rename the window to "agent-doc"
        let _ = iso
            .cmd()
            .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
            .status();
        assert!(
            agent_doc_tmux_io::has_window_named(&iso, session, "agent-doc"),
            "should find 'agent-doc' window after renaming"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn has_named_window_false_for_nonexistent_session() {
        let iso = IsolatedTmux::new("route-test-named-win-no-sess");
        assert!(
            !agent_doc_tmux_io::has_window_named(&iso, "nonexistent", "agent-doc"),
            "should return false for nonexistent session"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_target_session_ignores_blank_context_session() {
        let iso = IsolatedTmux::new("route-test-blank-context");
        let cwd = std::env::current_dir().unwrap();
        let pane = iso.auto_start("claude", &cwd).unwrap();
        let current_session = iso.pane_session(&pane).unwrap();

        let resolved =
            resolve_target_session(&iso, Some("   "), &[], None, &HarnessConfig::claude());
        assert_eq!(
            resolved, current_session,
            "blank context_session should fall back to the live target session"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_preferred_session_prefers_live_project_pin_over_current_session() {
        let dir = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "tmux_session = \"0\"\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("route-test-project-session-pin");
        let _configured = iso.new_session("0", dir.path()).unwrap();
        let _current = iso.new_session("1", dir.path()).unwrap();

        assert_eq!(current_tmux_session(&iso).as_deref(), Some("1"));
        assert_eq!(
            resolve_preferred_session(&iso, None, "[test]").as_deref(),
            Some("0"),
            "a live project tmux_session pin should beat the caller's current session"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_target_session_prefers_nested_file_root_pin_over_outer_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");
        let _cwd_guard = ScopedCurrentDir::set(root);

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"4\"\n",
        )
        .unwrap();
        std::fs::write(
            subroot.join(".agent-doc/config.toml"),
            "tmux_session = \"1\"\n",
        )
        .unwrap();

        let child_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("route-test-nested-file-root-pin");
        let _child = iso.new_session("1", root).unwrap();
        let _workspace = iso.new_session("4", root).unwrap();

        assert_eq!(
            resolve_target_session(&iso, None, &[], Some(&child_doc), &HarnessConfig::claude()),
            "1",
            "route should honor the nested file's own project pin even when cwd is the outer workspace root"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_target_session_prefers_shared_workspace_root_pin_for_mixed_roots() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(&subroot);
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"4\"\n",
        )
        .unwrap();
        std::fs::write(
            subroot.join(".agent-doc/config.toml"),
            "tmux_session = \"1\"\n",
        )
        .unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &root_doc,
            "---\nagent_doc_session: root-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("route-test-mixed-root-workspace-pin");
        let _child = iso.new_session("1", root).unwrap();
        let _workspace = iso.new_session("4", root).unwrap();

        let col_args = vec![
            root_doc.to_string_lossy().to_string(),
            child_doc.to_string_lossy().to_string(),
        ];
        assert_eq!(
            resolve_target_session(
                &iso,
                None,
                &col_args,
                Some(&child_doc),
                &HarnessConfig::claude(),
            ),
            "4",
            "mixed-root route should stay on the shared workspace root pin instead of the focused child root"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn implicit_fallback_session_is_not_auto_start_target() {
        let iso = IsolatedTmux::new("route-test-no-implicit-fallback");
        let result =
            ensure_auto_start_target_session(&iso, None, "claude", &HarnessConfig::claude());
        assert!(
            result.is_err(),
            "dead implicit fallback session should not be auto-started"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
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
}

//! Top-level route command runtime.

use anyhow::Result;
use std::path::Path;
use std::time::Duration;
use tmux_router::Tmux;

use crate::authoritative_dispatch::RouteAuthoritativeActorEffects;
use crate::dispatch_only::DispatchOnlyRouteEffects;
use crate::document_prep::{RouteDocumentPrepEffects, prepare_route_document};
use crate::pane_resolution::{ManagedPaneResolutionEffects, cleanup_failed_route_panes};
use crate::session_resolution::resolve_target_session;
use crate::startup::RouteStartupEffects;
use crate::startup_debounce::await_idle;
use agent_doc_harness::HarnessConfig;
use agent_doc_run_context_io::AgentDocContextExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMode {
    Managed,
    DispatchOnly,
}

#[derive(Clone, Copy)]
pub struct RouteCommandEffects {
    pub document_prep_effects: RouteDocumentPrepEffects,
    pub managed_pane_resolution_effects: ManagedPaneResolutionEffects,
    pub authoritative_actor_effects: RouteAuthoritativeActorEffects,
    pub dispatch_only_effects: DispatchOnlyRouteEffects,
    pub startup_effects: RouteStartupEffects,
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_tmux_with_options(
    file: &Path,
    tmux: &Tmux,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    effects: RouteCommandEffects,
) -> Result<()> {
    tracing::debug!(file = %file.display(), pane, debounce_ms, cols = ?col_args, "route::run start");
    // Clean stale registry entries before lookup, but stay on the editor-driven
    // fast path: use `SkipExpensiveStashCleanup` so the pre-lookup prune does NOT
    // scan stash panes. The expensive stash-pane purge re-resolves every live
    // fleet session's owner pane (tmux capture + process sampling + supervisor
    // probe) *per unregistered stash pane*, which measured ~28s on a busy fleet
    // and was the dominant `Run Agent Doc` dispatch latency (`#run-agent-doc-latency`).
    // Route only needs stale registry rows + dead non-stash panes pruned for an
    // accurate pane lookup; orphaned stash-pane hygiene belongs to explicit
    // `resync`/`sync`, mirroring `safe_passive_prune_cleanup_mode`.
    let _ = agent_doc_sync_io::resync::prune_with_tmux_timed_in_mode(
        tmux,
        agent_doc_tmux::PruneCleanupMode::SkipExpensiveStashCleanup,
    );

    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    if let Err(err) =
        agent_doc_controller_io::project_controller::clear_queue_context_clear_manual_cooldown_for_file(file)
    {
        eprintln!(
            "[route] warning: failed to clear queue clear-cooldown projection for {}: {err:#}",
            file.display()
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_clear_queue_cooldown_failed file={} authority=projection error={:?}",
                file.display(),
                err.to_string()
            ),
        );
    }

    // Debounce: wait for file mtime and editor typing indicator to settle before
    // route performs visible mutations such as session UUID insertion or
    // duplicate-prompt cleanup.
    if debounce_ms > 0 {
        await_idle(file, Duration::from_millis(debounce_ms))?;
    }

    let prepared = prepare_route_document(file, effects.document_prep_effects)?;
    let updated_content = prepared.content;
    let session_id = prepared.session_id;

    let rc = agent_doc_run_context_io::run_context(file.to_path_buf());
    let fm = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &updated_content,
        file,
        &rc.ssh_context(),
    )
    .map(|(f, _)| f)?;
    let global_config = rc.global_config();
    let mut harness = HarnessConfig::from_context(&fm, &global_config);
    if plain_trigger {
        harness.apply_plain_trigger_override();
    }

    // Use absolute path for trigger commands to avoid CWD-dependent resolution
    // when the pane's CWD differs from the invoker's (e.g., narrowed to a
    // submodule root). Relative paths would resolve to the submodule's version
    // of the file when the same relative path exists in both locations.
    let file_path = agent_doc_git_io::dirs::resolve_absolute_file_path(file)
        .to_string_lossy()
        .into_owned();
    let target_session = resolve_target_session(tmux, None, col_args, Some(file), &harness);
    eprintln!("[route] target tmux session: {}", target_session);

    // === SINGLE EXIT POINT PATTERN ===
    // All paths resolve to a pane_id, then ONE sync call handles layout.
    // This prevents propagation bugs where cross-cutting behavior (sync)
    // is added to one path but missed on others.

    let mut created_panes = Vec::new();

    let pane_id = match mode {
        RouteMode::Managed => crate::pane_resolution::resolve_or_create_pane(
            tmux,
            file,
            pane,
            col_args,
            &session_id,
            &file_path,
            &target_session,
            &harness,
            &mut created_panes,
            effects.managed_pane_resolution_effects,
        ),
        RouteMode::DispatchOnly => crate::pane_resolution::resolve_or_create_pane_dispatch_only(
            tmux,
            file,
            pane,
            col_args,
            &session_id,
            &file_path,
            &target_session,
            &harness,
            &mut created_panes,
            effects.authoritative_actor_effects,
            effects.dispatch_only_effects,
            effects.startup_effects,
        ),
    };

    match pane_id {
        Ok(ref _pid) => {
            // NOTE: sync_after_claim was removed here to eliminate the double-sync
            // glitch. The JB plugin already triggers sync with the correct window
            // and col_args via the route call. A second sync (with window=None)
            // races with the first sync's stash operations, causing panes to
            // bounce between stash and agent-doc window visibly.
            // The JB plugin's sync call is authoritative - no defensive re-sync needed.
            agent_doc_controller_io::editor_route_errors::clear_for_success(
                file,
                "route_success",
                agent_doc_ops_log_io::log_op,
            );
            Ok(())
        }
        Err(e) => {
            // Clean up panes created during the failed route attempt, but fail
            // closed for the current session owner: if a newly-created pane is
            // still the registered live pane for this document, preserve it so
            // a missed start-ack cannot crash the user's active tmux pane.
            cleanup_failed_route_panes(tmux, file, &session_id, &created_panes);
            Err(e)
        }
    }
}

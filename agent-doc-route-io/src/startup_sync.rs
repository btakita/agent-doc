//! Route post-claim sync helpers.

use tmux_router::Tmux;

/// A route needs a second layout pass only when it created the pane that became
/// the dispatch target and the editor supplied an actual split projection.
/// Existing-pane routes were already reconciled by the command plane and must
/// not re-run sync, or they can bounce panes between the working and stash
/// windows.
pub fn newly_provisioned_route_needs_reconcile(
    pane_id: &str,
    created_panes: &[String],
    col_args: &[String],
) -> bool {
    col_args.len() >= 2 && created_panes.iter().any(|created| created == pane_id)
}

/// Reconcile the exact editor projection after route provisioning registered a
/// pane that did not exist during the command plane's pre-route sync.
pub fn reconcile_newly_provisioned_route(
    tmux: &Tmux,
    pane_id: &str,
    created_panes: &[String],
    col_args: &[String],
) {
    if newly_provisioned_route_needs_reconcile(pane_id, created_panes, col_args) {
        sync_after_claim(tmux, pane_id, col_args);
    }
}

/// After a lazy claim, sync tmux layout for all files in the same window.
pub fn sync_after_claim(tmux: &Tmux, pane_id: &str, col_args: &[String]) {
    let Some(window_id) = agent_doc_tmux_io::target_window_id(tmux, pane_id) else {
        return;
    };

    let effective_col_args: Vec<String> = if !col_args.is_empty() {
        col_args.to_vec()
    } else {
        let registry = match agent_doc_session_registry_io::load() {
            Ok(r) => r,
            Err(_) => return,
        };

        registry
            .values()
            .filter(|entry| {
                !entry.pane.is_empty()
                    && tmux.pane_alive(&entry.pane)
                    && agent_doc_tmux_io::target_window_id(tmux, &entry.pane).as_deref()
                        == Some(&window_id)
                    && !entry.file.is_empty()
            })
            .map(|entry| entry.file.clone())
            .collect()
    };

    if effective_col_args.len() < 2 {
        return;
    }

    let file_count = effective_col_args.len();
    let project_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let actor_bindings = effective_col_args
        .iter()
        .filter_map(|file_arg| {
            let file = std::path::PathBuf::from(file_arg);
            let file = if file.is_absolute() {
                file
            } else {
                project_root.join(file)
            };
            let entry = agent_doc_session_registry_io::lookup_file_entry_in(&project_root, &file)
                .ok()
                .flatten()?;
            tmux.pane_alive(&entry.pane).then_some(
                agent_doc_controller_io::project_controller::ControllerTmuxActorBinding {
                    document_path: file.display().to_string(),
                    session_id: entry.session_id,
                    pane_id: entry.pane,
                    generation: 0,
                },
            )
        })
        .collect::<Vec<_>>();
    if let Err(e) = agent_doc_sync_io::sync::run_layout_only_exact_visible_with_actor_bindings_and_tmux_in_project_root(
        &project_root,
        &effective_col_args,
        Some(&window_id),
        None,
        &actor_bindings,
        tmux,
    ) {
        eprintln!("[route] warning: post-claim sync failed: {}", e);
    } else {
        eprintln!(
            "[route] Auto-synced {} files in window {}",
            file_count, window_id
        );
    }
}

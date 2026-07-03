//! Route post-claim sync helpers.

use tmux_router::Tmux;

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
    if let Err(e) =
        agent_doc_sync_io::sync::run_with_tmux(&effective_col_args, Some(&window_id), None, tmux)
    {
        eprintln!("[route] warning: post-claim sync failed: {}", e);
    } else {
        eprintln!(
            "[route] Auto-synced {} files in window {}",
            file_count, window_id
        );
    }
}

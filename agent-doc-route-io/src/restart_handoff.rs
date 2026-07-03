//! Route restart handoff polling.

use crate::dispatch_target::register_dispatch_target;
use std::path::Path;
use std::time::{Duration, Instant};
use tmux_router::Tmux;

pub fn wait_for_busy_restart_handoff(
    tmux: &Tmux,
    file: &Path,
    file_path: &str,
    session_id: &str,
    previous_pane: &str,
) {
    let registry_base_dir =
        agent_doc_session_registry_io::dispatch_registry::registry_base_dir_for_dispatch(file_path);
    let timeout = if cfg!(test) {
        Duration::from_secs(20)
    } else {
        Duration::from_secs(5)
    };
    let poll = Duration::from_millis(100);
    let start = Instant::now();
    let mut handed_off_pane: Option<String> = None;
    while start.elapsed() < timeout {
        if let Ok(registry) = agent_doc_session_registry_io::load_in(&registry_base_dir)
            && let Some(entry) = registry
                .values()
                .find(|entry| entry.session_id == session_id)
            && !entry.pane.is_empty()
        {
            if entry.pane != previous_pane {
                handed_off_pane = Some(entry.pane.clone());
                if agent_doc_sync_io::sync::find_normal_path_owner_pane(tmux, file, session_id)
                    .as_deref()
                    == Some(entry.pane.as_str())
                {
                    eprintln!(
                        "[route] supervisor restart handed {} from pane {} to authoritative pane {} before retry",
                        file_path, previous_pane, entry.pane
                    );
                    return;
                }
            } else {
                handed_off_pane = None;
            }
        }
        match agent_doc_tmux::resolve_associated_panes(
            agent_doc_sync_io::sync::find_associated_panes(tmux, file, session_id),
            None,
        ) {
            agent_doc_tmux::AssociatedPaneResolution::Selected { winner, .. }
                if winner.pane_id != previous_pane && !winner.is_stash() =>
            {
                if let Err(err) =
                    register_dispatch_target(tmux, session_id, &winner.pane_id, file_path)
                {
                    eprintln!(
                        "[route] warning: failed to project restart handoff pane {} into the registry for {}: {}",
                        winner.pane_id, file_path, err
                    );
                }
                eprintln!(
                    "[route] supervisor restart for {} has not refreshed the registry yet, but a unique associated pane {} is alive via {} — adopting it as the handoff target before retry",
                    file_path,
                    winner.pane_id,
                    winner.source_summary()
                );
                return;
            }
            _ => {}
        }
        std::thread::sleep(poll);
    }
    if let Some(pane) = handed_off_pane {
        eprintln!(
            "[route] supervisor restart handed {} from pane {} to authoritative pane {} before retry, but live-owner proof is still catching up",
            file_path, previous_pane, pane
        );
    }
}

//! Route dispatch target registry binding.

use anyhow::Result;
use std::path::Path;
use tmux_router::Tmux;

pub fn register_dispatch_target(
    tmux: &Tmux,
    session_id: &str,
    pane_id: &str,
    file_path: &str,
) -> Result<()> {
    let requested = agent_doc_session_registry_io::dispatch_registry::canonical_dispatch_file(
        Path::new(file_path),
    );
    let requested_str = requested.to_string_lossy().to_string();
    let base_dir = agent_doc_session_registry_io::dispatch_registry::registry_base_dir_for_dispatch(
        &requested_str,
    );
    agent_doc_session_registry_io::dispatch_registry::ensure_dispatch_target_can_bind_file(
        &base_dir,
        pane_id,
        &requested_str,
        |entry, registered| {
            agent_doc_sync_io::sync::find_normal_path_owner_pane(
                tmux,
                registered,
                &entry.session_id,
            )
            .as_deref()
                == Some(pane_id)
        },
    )?;
    let window = agent_doc_tmux_io::target_window_id(tmux, pane_id).unwrap_or_default();
    let cwd = base_dir.to_string_lossy().to_string();
    agent_doc_session_registry_io::registration::register_full_with_cwd_in(
        &base_dir,
        session_id,
        pane_id,
        &requested_str,
        std::process::id(),
        &window,
        &cwd,
    )
}

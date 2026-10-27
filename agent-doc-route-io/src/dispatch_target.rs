//! Route dispatch target registry binding.

use anyhow::Result;
use std::path::Path;
use tmux_router::Tmux;

pub use agent_doc_session_registry_io::registration::PostLaunchDispatchBind;

/// The canonical document path and registry root a dispatch bind writes to,
/// after refusing a cross-file rebind of `pane_id`.
fn guarded_dispatch_bind_target(
    tmux: &Tmux,
    pane_id: &str,
    file_path: &str,
) -> Result<(String, std::path::PathBuf)> {
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
    Ok((requested_str, base_dir))
}

/// Bind `pane_id` as `file_path`'s dispatch target before the route has
/// launched or restarted the pane's owner: a plain `route`/`dispatch_bind`
/// projection that re-projects the actor as `Ready`.
pub fn register_dispatch_target(
    tmux: &Tmux,
    session_id: &str,
    pane_id: &str,
    file_path: &str,
) -> Result<()> {
    let (requested_str, base_dir) = guarded_dispatch_bind_target(tmux, pane_id, file_path)?;
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

/// Bind `pane_id` as `file_path`'s dispatch target after the route launched or
/// restarted the document's owner (`#dispatchrebindaudit`). From the launch
/// on, the owner (the child `agent-doc start` or the restarted supervisor)
/// owns the actor generation: this writes only over a projection binding,
/// fenced on the generation it decided against, and never over the owner's
/// record. See
/// [`agent_doc_session_registry_io::registration::bind_dispatch_target_after_owner_launch_in`].
pub fn bind_dispatch_target_after_owner_launch(
    tmux: &Tmux,
    session_id: &str,
    pane_id: &str,
    file_path: &str,
) -> Result<PostLaunchDispatchBind> {
    let (requested_str, base_dir) = guarded_dispatch_bind_target(tmux, pane_id, file_path)?;
    let window = agent_doc_tmux_io::target_window_id(tmux, pane_id).unwrap_or_default();
    let cwd = base_dir.to_string_lossy().to_string();
    agent_doc_session_registry_io::registration::bind_dispatch_target_after_owner_launch_in(
        &base_dir,
        session_id,
        pane_id,
        &requested_str,
        std::process::id(),
        &window,
        &cwd,
    )
}

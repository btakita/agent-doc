//! Route dispatch target registry binding.

use anyhow::Result;
use std::path::Path;
use tmux_router::Tmux;

pub use agent_doc_session_registry_io::registration::{
    PostLaunchDispatchBind, PreDispatchDispatchBind,
};

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
/// projection that re-projects the actor as `Ready`. A pane whose owner is
/// already running binds with [`bind_running_dispatch_target`] instead, which
/// never writes `Ready` over that owner (`#predispatchreadyoverwrite`).
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

/// Bind `pane_id` as `file_path`'s dispatch target before the route dispatches
/// into a pane whose owner is already running (`#predispatchreadyoverwrite`).
///
/// The record already binding this session to `pane_id` in a live state is
/// left exactly as the owner wrote it (no `Ready` over `Busy`/`Starting`);
/// only the registry metadata is refreshed. An owner on another live pane is
/// never bound over: the route fails closed instead of bumping the generation
/// and retiring that owner's supervisor. See
/// [`agent_doc_session_registry_io::registration::bind_running_dispatch_target_in`].
pub fn bind_running_dispatch_target(
    tmux: &Tmux,
    session_id: &str,
    pane_id: &str,
    file_path: &str,
) -> Result<PreDispatchDispatchBind> {
    let (requested_str, base_dir) = guarded_dispatch_bind_target(tmux, pane_id, file_path)?;
    let window = agent_doc_tmux_io::target_window_id(tmux, pane_id).unwrap_or_default();
    let cwd = base_dir.to_string_lossy().to_string();
    let outcome = agent_doc_session_registry_io::registration::bind_running_dispatch_target_in(
        &base_dir,
        session_id,
        pane_id,
        &requested_str,
        std::process::id(),
        &window,
        &cwd,
        &|owner_pane| tmux.pane_alive(owner_pane),
    )?;
    let file = Path::new(file_path);
    match &outcome {
        PreDispatchDispatchBind::OwnerStatePreserved { state, generation } => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_pre_dispatch_bind_owner_state_preserved file={} pane={} state={} generation={}",
                    file_path,
                    pane_id,
                    state.as_str(),
                    generation
                ),
            );
        }
        PreDispatchDispatchBind::Bound { .. } => {}
        PreDispatchDispatchBind::LiveOwnerElsewhere {
            pane_id: owner_pane,
            generation,
            state,
            caller,
            reason,
        } => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_pre_dispatch_bind_live_owner_elsewhere file={} pane={} owner_pane={} owner_generation={} owner_state={} owner_caller={} owner_reason={}",
                    file_path,
                    pane_id,
                    owner_pane,
                    generation,
                    state.as_str(),
                    caller,
                    reason
                ),
            );
            anyhow::bail!(
                "{caller}/{reason} owns {file_path} at generation {generation} ({}) on live pane {owner_pane}; route will not bind pane {pane_id} over that owner. Run `agent-doc {file_path}` from pane {owner_pane}, or `agent-doc claim {file_path}` to move it deliberately",
                state.as_str()
            );
        }
    }
    Ok(outcome)
}

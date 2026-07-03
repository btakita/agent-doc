//! Route pane provenance observations.

use tmux_router::Tmux;

pub fn pane_route_provenance(tmux: &Tmux, pane_id: &str) -> String {
    let pane_pid = agent_doc_tmux_io::pane_pid(tmux, pane_id)
        .map(|pid| pid.to_string())
        .unwrap_or_else(|| "?".to_string());
    let pane_session =
        agent_doc_tmux_io::target_session_name(tmux, pane_id).unwrap_or_else(|| "?".to_string());
    let current_command =
        agent_doc_tmux_io::target_current_command(tmux, pane_id).unwrap_or_else(|| "?".to_string());
    format!(
        "pane={} pane_pid={} pane_session={} current_command={}",
        pane_id, pane_pid, pane_session, current_command
    )
}

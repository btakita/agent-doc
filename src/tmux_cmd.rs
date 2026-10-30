//! Headless tmux bootstrap command adapter.

use agent_doc_config::terminal_host::ResolvedTerminalHost;
use anyhow::Result;
use std::path::Path;

/// Nonblocking editor-facing start. The supervisor becomes the tmux pane
/// process; this CLI process always returns after dispatch.
pub fn start(file: &Path, client_id: Option<&str>) -> Result<()> {
    let resume = agent_doc_harness::ResumeRequest::Latest;
    let outcome = agent_doc_start_io::start_in_tmux_pane_for_client(
        file,
        false,
        false,
        agent_doc_supervisor::route_owned::RouteOwnedReapPolicy::Auto,
        agent_doc_supervisor::route_owned::RouteOwnedStartPurpose::Dispatch,
        Some(&resume),
        None,
        client_id,
    )?;
    println!(
        "started agent-doc for {} in tmux session '{}' pane {}",
        file.display(), outcome.session_name, outcome.pane_id,
    );
    Ok(())
}

pub fn ensure(
    file: &Path,
    session: Option<&str>,
    json: bool,
    ide_terminal: bool,
    client_id: Option<&str>,
) -> Result<()> {
    let outcome = if ide_terminal {
        // `#ideriderindependentnav` P3: the editor's surface client id selects
        // its navigation lane session; without one (or in `shared` mode) this
        // is the shared ensure.
        agent_doc_start_io::ensure_tmux_session_for_ide_client(file, session, client_id)?
    } else {
        agent_doc_start_io::ensure_tmux_session(file, session)?
    };
    let terminal_host = match outcome.terminal_host {
        ResolvedTerminalHost::Ide => "ide",
        ResolvedTerminalHost::External => "external",
        ResolvedTerminalHost::None => "none",
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_name": outcome.session_name,
                "pane_id": outcome.pane_id,
                "attach_command": outcome.attach_command,
                "created": outcome.created,
                "attached": outcome.attached,
                "resolution": outcome.resolution,
                "document_pane": outcome.document_pane,
                "terminal_host": terminal_host,
                "terminal_host_reason": outcome.terminal_host_reason,
                "auto_start_tmux": outcome.auto_start_tmux,
            }))?
        );
    } else {
        println!("session: {}", outcome.session_name);
        println!("pane: {}", outcome.pane_id);
        println!("created: {}", outcome.created);
        println!("attached: {}", outcome.attached);
        println!("attach: {}", outcome.attach_command);
        println!("terminal host: {terminal_host}");
        println!("terminal host reason: {}", outcome.terminal_host_reason);
    }
    Ok(())
}

/// `agent-doc gc`'s navigation-lane pass (`#ideriderindependentnav` P5): fold
/// lane panes back after `editor_navigation` is switched to `shared`, and close
/// empty lane sessions agent-doc created. Never touches an unmarked session.
pub fn gc_navigation_lanes(root: Option<&Path>, dry_run: bool) -> Result<()> {
    use agent_doc_editor_surface::LaneReconcileAction;
    let project_root = match root {
        Some(root) => root.to_path_buf(),
        None => agent_doc_project_root_io::project_root_from_cwd()?,
    };
    let tmux = tmux_router::Tmux::default_server();
    let report = agent_doc_controller_io::project_controller::reconcile_navigation_lane_sessions(
        &tmux,
        &project_root,
        &Default::default(),
        dry_run,
    );
    if report.actions.is_empty() {
        return Ok(());
    }
    let verb = if dry_run { "would " } else { "" };
    for action in &report.actions {
        match action {
            LaneReconcileAction::FoldPane { session, pane } => {
                eprintln!(
                    "[gc] navigation lane {session}: {verb}fold pane {pane} back into the project session"
                );
            }
            LaneReconcileAction::Close { session } => {
                eprintln!("[gc] navigation lane {session}: {verb}close (empty)");
            }
            LaneReconcileAction::Retain { session, reason } => {
                eprintln!("[gc] navigation lane {session}: kept ({})", reason.as_str());
            }
        }
    }
    if !dry_run {
        eprintln!(
            "[gc] navigation lanes ({}): {} pane(s) folded, {} session(s) closed",
            report.mode.as_str(),
            report.folded.len(),
            report.closed.len()
        );
        for failure in &report.failures {
            eprintln!("[gc] navigation lane warning: {failure}");
        }
    }
    Ok(())
}

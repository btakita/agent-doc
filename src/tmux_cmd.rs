//! Headless tmux bootstrap command adapter.

use agent_doc_config::terminal_host::ResolvedTerminalHost;
use anyhow::Result;
use std::path::Path;

pub fn ensure(file: &Path, session: Option<&str>, json: bool, ide_terminal: bool) -> Result<()> {
    let outcome = if ide_terminal {
        agent_doc_start_io::ensure_tmux_session_for_ide(file, session)?
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

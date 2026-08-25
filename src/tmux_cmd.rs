//! Headless tmux bootstrap command adapter.

use anyhow::Result;
use std::path::Path;

pub fn ensure(file: &Path, session: Option<&str>, json: bool) -> Result<()> {
    let outcome = agent_doc_start_io::ensure_tmux_session(file, session)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_name": outcome.session_name,
                "pane_id": outcome.pane_id,
                "attach_command": outcome.attach_command,
                "created": outcome.created,
                "resolution": outcome.resolution,
                "document_pane": outcome.document_pane,
            }))?
        );
    } else {
        println!("session: {}", outcome.session_name);
        println!("pane: {}", outcome.pane_id);
        println!("created: {}", outcome.created);
        println!("attach: {}", outcome.attach_command);
    }
    Ok(())
}

//! `agent-doc document`.
//!
//! Prints the session document the current tmux pane claims: the FILE the
//! no-argument harness entrypoint runs on. Harnesses whose admission is not a
//! `UserPromptSubmit` hook (Grok Build, OpenCode) run this to learn FILE; the
//! Claude Code and Codex hooks call the same resolver in-process.

use anyhow::Result;
use std::path::Path;

use agent_doc_session_registry_io::pane_document::resolve_pane_document;

pub fn run(pane: Option<&str>, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let pane = pane
        .map(str::to_string)
        .or_else(agent_doc_tmux_io::current_pane_id_from_env);
    let result = resolve_pane_document(&cwd, pane.as_deref());
    match result {
        Ok(file) => {
            let document = display(&cwd, &file);
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": true,
                        "pane": pane,
                        "document": document,
                        "path": file.display().to_string(),
                    })
                );
            } else {
                println!("{document}");
            }
            Ok(())
        }
        Err(refusal) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "pane": pane,
                        "kind": refusal.kind(),
                        "reason": refusal.to_string(),
                    })
                );
            }
            Err(anyhow::Error::new(refusal))
        }
    }
}

/// Relative to `cwd` when the document lives under it, else absolute.
fn display(cwd: &Path, file: &Path) -> String {
    file.strip_prefix(cwd).unwrap_or(file).display().to_string()
}

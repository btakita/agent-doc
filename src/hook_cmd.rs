//! # Module: hook_cmd
//!
//! CLI subcommands for the hook system.
//!
//! - `agent-doc hook fire <EVENT> <FILE> [--session-id ID] [--data JSON]` — fire an event
//! - `agent-doc hook poll <EVENT> [--since SECS] [--project-root PATH]` — poll for events
//! - `agent-doc hook listen [--project-root PATH]` — start hook socket listener
//! - `agent-doc hook gc [--project-root PATH]` — clean up expired events

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use agent_kit::hooks::{Event, HookRegistry};

/// Fire a hook event from the CLI.
///
/// Used by Claude Code hooks bridge:
/// ```json
/// { "hooks": { "PostToolUse": [{ "command": "agent-doc hook fire post_write $FILE" }] } }
/// ```
pub fn fire(
    event_name: &str,
    file: &str,
    session_id: Option<&str>,
    data: Option<&str>,
) -> Result<()> {
    let file_path = Path::new(file);
    let hooks_dir = agent_kit::hooks::hooks_dir_for_file(file_path)
        .context("could not find .agent-doc directory")?;

    let registry = HookRegistry::new(&hooks_dir);

    let session_id = session_id
        .map(|s| s.to_string())
        .or_else(|| crate::frontmatter::read_session_id(file_path))
        .unwrap_or_default();

    let data_value: serde_json::Value = data
        .and_then(|d| serde_json::from_str(d).ok())
        .unwrap_or(serde_json::json!(null));

    let event_id = registry.fire(
        event_name,
        Event {
            file: file.to_string(),
            session_id,
            data: data_value,
        },
    )?;

    println!("{}", event_id);
    Ok(())
}

/// Poll for hook events from the CLI.
pub fn poll(event_name: &str, since_secs: u64, project_root: Option<&str>) -> Result<()> {
    let hooks_dir = if let Some(root) = project_root {
        PathBuf::from(root).join(".agent-doc/hooks")
    } else {
        // Use CWD
        let cwd = std::env::current_dir()?;
        cwd.join(".agent-doc/hooks")
    };

    let registry = HookRegistry::new(&hooks_dir);
    let events = registry.poll(event_name, since_secs)?;

    let json = serde_json::to_string_pretty(
        &events
            .iter()
            .map(|e| {
                serde_json::json!({
                    "event_name": e.name,
                    "file": e.event.file,
                    "session_id": e.event.session_id,
                    "timestamp": e.timestamp,
                    "event_id": e.event_id,
                    "data": e.event.data,
                })
            })
            .collect::<Vec<_>>(),
    )?;

    println!("{}", json);
    Ok(())
}

/// Start a hook socket listener.
///
/// Listens on `.agent-doc/hooks.sock` for JSON messages from `SocketTransport`.
/// Each received event is written to the file-based hook directory for other sessions to poll.
#[cfg(unix)]
pub fn listen(project_root: Option<&str>) -> Result<()> {
    let root = if let Some(r) = project_root {
        PathBuf::from(r)
    } else {
        std::env::current_dir()?
    };

    let hooks_dir = root.join(".agent-doc/hooks");
    let sock_path = root.join(".agent-doc/hooks.sock");

    // Clean up stale socket
    if sock_path.exists() {
        let _ = std::fs::remove_file(&sock_path);
    }

    std::fs::create_dir_all(&hooks_dir)?;
    eprintln!("[hook-listen] starting on {}", sock_path.display());

    let listener = std::os::unix::net::UnixListener::bind(&sock_path)
        .with_context(|| format!("failed to bind hook socket: {}", sock_path.display()))?;

    eprintln!("[hook-listen] ready, waiting for events...");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                use std::io::{BufRead, BufReader, Write};
                let reader = BufReader::new(
                    stream
                        .try_clone()
                        .unwrap_or_else(|_| stream.try_clone().unwrap()),
                );
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    match handle_hook_message(trimmed, &hooks_dir) {
                        Ok(()) => {
                            let _ = stream.write_all(b"ok\n");
                            let _ = stream.flush();
                        }
                        Err(e) => {
                            eprintln!("[hook-listen] error: {}", e);
                            let _ = stream.write_all(b"error\n");
                            let _ = stream.flush();
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[hook-listen] accept error: {}", e);
            }
        }
    }

    Ok(())
}

/// Start a hook socket listener (not supported on non-Unix platforms).
#[cfg(not(unix))]
pub fn listen(_project_root: Option<&str>) -> Result<()> {
    anyhow::bail!("hook socket listener is only supported on Unix platforms")
}

/// Handle a single hook message from the socket.
fn handle_hook_message(msg: &str, hooks_dir: &Path) -> Result<()> {
    let json: serde_json::Value = serde_json::from_str(msg).context("invalid JSON")?;

    let event_name = json["event_name"]
        .as_str()
        .or_else(|| json["event"].as_str())
        .context("missing event_name")?;

    let file = json["file"].as_str().unwrap_or("");
    let session_id = json["session_id"].as_str().unwrap_or("");
    let data = json.get("data").cloned().unwrap_or(serde_json::json!(null));

    let registry = HookRegistry::new(hooks_dir);
    let event_id = registry.fire(
        event_name,
        Event {
            file: file.to_string(),
            session_id: session_id.to_string(),
            data,
        },
    )?;

    eprintln!(
        "[hook-listen] received {} for {} (id={})",
        event_name, file, event_id
    );
    Ok(())
}

/// Clean up expired events.
pub fn gc(project_root: Option<&str>) -> Result<()> {
    let root = if let Some(r) = project_root {
        PathBuf::from(r)
    } else {
        std::env::current_dir()?
    };

    let hooks_dir = root.join(".agent-doc/hooks");
    let registry = HookRegistry::new(&hooks_dir);
    let cleaned = registry.gc()?;
    println!("Cleaned {} hook directories", cleaned);
    Ok(())
}

//! # Module: hooks
//!
//! Integration with agent-kit's hook system for cross-session coordination.
//!
//! Fires events at key lifecycle points so other sessions can react:
//! - `post_write` — after agent-doc writes a response to a document
//! - `post_commit` — after agent-doc commits changes
//! - `claim` — when a document is claimed by a session
//! - `layout_change` — when tmux layout changes
//!
//! Best-effort: hook failures are logged but never block the main operation.

use std::path::Path;

use agent_kit::hooks::{Event, HookRegistry};

/// Fire a post_write hook event.
pub fn fire_post_write(file: &Path, session_id: &str, patch_count: usize) {
    if let Some(registry) = registry_for_file(file) {
        let _ = registry.fire("post_write", Event {
            file: file.to_string_lossy().into(),
            session_id: session_id.into(),
            data: serde_json::json!({"patches": patch_count}),
        }).map_err(|e| eprintln!("[hooks] post_write fire failed: {}", e));
    }
}

/// Fire a post_commit hook event.
pub fn fire_post_commit(file: &Path, session_id: &str) {
    if let Some(registry) = registry_for_file(file) {
        let _ = registry.fire("post_commit", Event {
            file: file.to_string_lossy().into(),
            session_id: session_id.into(),
            data: serde_json::json!(null),
        }).map_err(|e| eprintln!("[hooks] post_commit fire failed: {}", e));
    }
}

/// Fire a claim hook event.
pub fn fire_claim(file: &Path, session_id: &str, pane_id: &str) {
    if let Some(registry) = registry_for_file(file) {
        let _ = registry.fire("claim", Event {
            file: file.to_string_lossy().into(),
            session_id: session_id.into(),
            data: serde_json::json!({"pane": pane_id}),
        }).map_err(|e| eprintln!("[hooks] claim fire failed: {}", e));
    }
}

/// Fire a layout_change hook event.
pub fn fire_layout_change(file: &Path, session_id: &str, action: &str) {
    if let Some(registry) = registry_for_file(file) {
        let _ = registry.fire("layout_change", Event {
            file: file.to_string_lossy().into(),
            session_id: session_id.into(),
            data: serde_json::json!({"action": action}),
        }).map_err(|e| eprintln!("[hooks] layout_change fire failed: {}", e));
    }
}

/// Poll for new events on a named hook since the given timestamp.
pub fn poll(file: &Path, hook_name: &str, since_secs: u64) -> Vec<agent_kit::hooks::ReceivedEvent> {
    registry_for_file(file)
        .and_then(|r| r.poll(hook_name, since_secs).ok())
        .unwrap_or_default()
}

fn registry_for_file(file: &Path) -> Option<HookRegistry> {
    agent_kit::hooks::hooks_dir_for_file(file)
        .map(HookRegistry::new)
}

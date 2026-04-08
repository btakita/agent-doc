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

/// Execute document-level hooks for the given event.
///
/// Template vars `{{session_id}}`, `{{file}}`, `{{agent}}`, `{{model}}` are substituted
/// before each command is passed to `sh -c`. Best-effort: failures log to stderr only.
pub fn fire_doc_hooks(
    hooks: &std::collections::HashMap<String, Vec<String>>,
    event: &str,
    file: &Path,
    session_id: &str,
    agent: &Option<String>,
    model: &Option<String>,
) {
    let Some(cmds) = hooks.get(event) else { return };
    if cmds.is_empty() { return; }

    let file_str = file.to_string_lossy();
    let agent_str = agent.as_deref().unwrap_or("");
    let model_str = model.as_deref().unwrap_or("");

    for cmd_template in cmds {
        let cmd = cmd_template
            .replace("{{session_id}}", session_id)
            .replace("{{file}}", &file_str)
            .replace("{{agent}}", agent_str)
            .replace("{{model}}", model_str);

        eprintln!("[hooks] {} running: {}", event, cmd);
        match std::process::Command::new("sh").args(["-c", &cmd]).output() {
            Ok(output) if output.status.success() => {
                eprintln!("[hooks] {} ok", event);
            }
            Ok(output) => {
                eprintln!(
                    "[hooks] {} exited with code {:?}: {}",
                    event,
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Err(e) => {
                eprintln!("[hooks] {} failed to spawn: {}", event, e);
            }
        }
    }
}

/// Read frontmatter from file and fire document-level hooks for the given event.
///
/// Best-effort: if frontmatter cannot be read or hooks are empty, silently returns.
pub fn fire_doc_event(file: &Path, event: &str) {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return,
    };
    let (fm, _) = match crate::frontmatter::parse(&content) {
        Ok(r) => r,
        Err(_) => return,
    };
    if fm.hooks.is_empty() { return; }
    let session_id = fm.session.as_deref().unwrap_or("").to_string();
    fire_doc_hooks(&fm.hooks, event, file, &session_id, &fm.agent, &fm.model);
}

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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn poll(file: &Path, hook_name: &str, since_secs: u64) -> Vec<agent_kit::hooks::ReceivedEvent> {
    registry_for_file(file)
        .and_then(|r| r.poll(hook_name, since_secs).ok())
        .unwrap_or_default()
}

fn registry_for_file(file: &Path) -> Option<HookRegistry> {
    agent_kit::hooks::hooks_dir_for_file(file)
        .map(HookRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn fire_doc_hooks_substitutes_all_vars() {
        let tmp = std::env::temp_dir().join(format!("agent-doc-hooks-test-{}.txt", std::process::id()));
        let cmd = format!("echo '{{{{session_id}}}}:{{{{file}}}}:{{{{agent}}}}:{{{{model}}}}' > {}", tmp.display());
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("post_write".to_string(), vec![cmd]);
        fire_doc_hooks(&hooks, "post_write", Path::new("/my/doc.md"), "sid-1", &Some("claude".to_string()), &Some("opus".to_string()));
        let output = std::fs::read_to_string(&tmp).unwrap_or_default();
        assert!(output.contains("sid-1"), "session_id missing: {}", output);
        assert!(output.contains("/my/doc.md"), "file missing: {}", output);
        assert!(output.contains("claude"), "agent missing: {}", output);
        assert!(output.contains("opus"), "model missing: {}", output);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn fire_doc_hooks_noop_for_unknown_event() {
        let hooks: HashMap<String, Vec<String>> = HashMap::new();
        // must not panic
        fire_doc_hooks(&hooks, "post_commit", Path::new("/doc.md"), "id", &None, &None);
    }

    #[test]
    fn fire_doc_event_noop_for_nonexistent_file() {
        // must not panic for a file that doesn't exist
        fire_doc_event(Path::new("/nonexistent/path/doc.md"), "post_write");
    }

    #[test]
    fn fire_doc_event_noop_when_hooks_empty() {
        let tmp = std::env::temp_dir().join(format!("agent-doc-event-test-{}.md", std::process::id()));
        std::fs::write(&tmp, "---\nsession: abc\n---\nBody\n").unwrap();
        // No hooks in frontmatter — must not panic
        fire_doc_event(&tmp, "post_write");
        let _ = std::fs::remove_file(&tmp);
    }
}

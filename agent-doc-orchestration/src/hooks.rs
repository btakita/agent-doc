//! # Module: hooks
//!
//! Integration with agent-kit's hook system for cross-session coordination.
//!
//! Fires events at key lifecycle points so other sessions can react:
//! - `post_write` — after agent-doc writes a response to a document
//! - `post_commit` — after agent-doc commits changes
//! - `claim` — when a document is claimed by a session
//! - `layout_change` — when tmux layout changes
//! - `post_write` / `post_commit` include `capture_id` and `response_sha256`
//!   when a durable response capture exists for the current cycle.
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
    if cmds.is_empty() {
        return;
    }

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
    if fm.hooks.is_empty() {
        return;
    }
    let session_id = fm.session.as_deref().unwrap_or("").to_string();
    let harness = agent_doc_core::model_tier::detect_harness();
    let model_config = agent_doc_core::model_tier::ModelConfig::default();
    let resolved_model = fm
        .resolve_harness_model(&harness)
        .map(|s| agent_doc_core::model_tier::canonical_model_name(s, &harness, &model_config));
    fire_doc_hooks(
        &fm.hooks,
        event,
        file,
        &session_id,
        &fm.agent,
        &resolved_model,
    );
}

/// Fire a post_write hook event.
pub fn fire_post_write(file: &Path, session_id: &str, patch_count: usize) {
    if let Some(registry) = registry_for_file(file) {
        let mut data = serde_json::json!({"patches": patch_count});
        if let Some(meta) = capture_metadata(file)
            && let Some(obj) = data.as_object_mut()
        {
            obj.extend(meta);
        }
        let _ = registry
            .fire(
                "post_write",
                Event {
                    file: file.to_string_lossy().into(),
                    session_id: session_id.into(),
                    data,
                },
            )
            .map_err(|e| eprintln!("[hooks] post_write fire failed: {}", e));
    }
}

/// Fire a post_commit hook event.
pub fn fire_post_commit(file: &Path, session_id: &str) {
    if let Some(registry) = registry_for_file(file) {
        let data = capture_metadata(file)
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::json!(null));
        let _ = registry
            .fire(
                "post_commit",
                Event {
                    file: file.to_string_lossy().into(),
                    session_id: session_id.into(),
                    data,
                },
            )
            .map_err(|e| eprintln!("[hooks] post_commit fire failed: {}", e));
    }
    capture_tsift_memory_closeout(file);
}

/// Fire a claim hook event.
#[allow(dead_code)]
pub fn fire_claim(file: &Path, session_id: &str, pane_id: &str) {
    if let Some(registry) = registry_for_file(file) {
        let _ = registry
            .fire(
                "claim",
                Event {
                    file: file.to_string_lossy().into(),
                    session_id: session_id.into(),
                    data: serde_json::json!({"pane": pane_id}),
                },
            )
            .map_err(|e| eprintln!("[hooks] claim fire failed: {}", e));
    }
}

/// Fire a layout_change hook event.
#[allow(dead_code)]
pub fn fire_layout_change(file: &Path, session_id: &str, action: &str) {
    if let Some(registry) = registry_for_file(file) {
        let _ = registry
            .fire(
                "layout_change",
                Event {
                    file: file.to_string_lossy().into(),
                    session_id: session_id.into(),
                    data: serde_json::json!({"action": action}),
                },
            )
            .map_err(|e| eprintln!("[hooks] layout_change fire failed: {}", e));
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
    agent_kit::hooks::hooks_dir_for_file(file).map(HookRegistry::new)
}

fn capture_metadata(file: &Path) -> Option<serde_json::Map<String, serde_json::Value>> {
    let capture = crate::capture::load_active(file).ok().flatten()?;
    let mut map = serde_json::Map::new();
    map.insert(
        "capture_id".to_string(),
        serde_json::Value::String(capture.capture_id),
    );
    map.insert(
        "response_sha256".to_string(),
        serde_json::Value::String(capture.response_sha256),
    );
    Some(map)
}

fn capture_tsift_memory_closeout(file: &Path) {
    let capture = match crate::capture::load_active(file) {
        Ok(Some(capture)) => capture,
        Ok(None) => return,
        Err(err) => {
            eprintln!("[hooks] tsift-memory closeout capture skipped: {err}");
            return;
        }
    };
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return;
    };
    if !project_root.join(".tsift/memory.db").exists() {
        return;
    }
    let response_summary = summarize_agent_doc_response(&capture.response_body);
    if response_summary.trim().is_empty() {
        eprintln!(
            "[hooks] tsift-memory closeout capture skipped for {}: empty response body",
            file.display()
        );
        return;
    }
    let prompt_target = extract_agent_doc_prompt_target(&capture.response_body)
        .unwrap_or_else(|| canonical.display().to_string());
    let commit_hash = git_head(&project_root).unwrap_or_else(|| "unknown".to_string());
    let output = std::process::Command::new("tsift")
        .arg("memory")
        .arg("capture-agent-doc-closeout")
        .arg(&project_root)
        .arg("--session-path")
        .arg(&canonical)
        .arg("--prompt-target")
        .arg(prompt_target)
        .arg("--response-summary")
        .arg(response_summary)
        .arg("--commit-hash")
        .arg(commit_hash)
        .arg("--session-check-status")
        .arg("committed")
        .arg("--json")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            eprintln!(
                "[hooks] tsift-memory closeout capture ok for {}",
                file.display()
            );
        }
        Ok(output) => {
            eprintln!(
                "[hooks] tsift-memory closeout capture exited with code {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Err(err) => {
            eprintln!("[hooks] tsift-memory closeout capture failed to spawn: {err}");
        }
    }
}

fn git_head(project_root: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!head.is_empty()).then_some(head)
}

fn extract_agent_doc_prompt_target(response_body: &str) -> Option<String> {
    for line in response_body.lines() {
        let without_hashes = line.trim().trim_start_matches('#').trim_start();
        let Some(rest) = without_hashes.strip_prefix("Re:") else {
            continue;
        };
        let target = rest.split(" — ").next().unwrap_or(rest).trim();
        if !target.is_empty() {
            return Some(target.to_string());
        }
    }
    None
}

fn summarize_agent_doc_response(response_body: &str) -> String {
    let lines = response_body
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != "<!-- patch:exchange -->"
                && trimmed != "<!-- /patch:exchange -->"
                && !(trimmed.starts_with("<!--") && trimmed.ends_with("-->"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    truncate_agent_doc_summary(lines.trim(), 4000)
}

fn truncate_agent_doc_summary(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let mut output = String::new();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return input.to_string();
        };
        output.push(ch);
    }
    if chars.next().is_some() {
        output.push_str("\n[truncated]");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn fire_doc_hooks_substitutes_all_vars() {
        let tmp =
            std::env::temp_dir().join(format!("agent-doc-hooks-test-{}.txt", std::process::id()));
        let cmd = format!(
            "echo '{{{{session_id}}}}:{{{{file}}}}:{{{{agent}}}}:{{{{model}}}}' > {}",
            tmp.display()
        );
        let mut hooks: HashMap<String, Vec<String>> = HashMap::new();
        hooks.insert("post_write".to_string(), vec![cmd]);
        fire_doc_hooks(
            &hooks,
            "post_write",
            Path::new("/my/doc.md"),
            "sid-1",
            &Some("claude".to_string()),
            &Some("opus".to_string()),
        );
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
        fire_doc_hooks(
            &hooks,
            "post_commit",
            Path::new("/doc.md"),
            "id",
            &None,
            &None,
        );
    }

    #[test]
    fn fire_doc_event_noop_for_nonexistent_file() {
        // must not panic for a file that doesn't exist
        fire_doc_event(Path::new("/nonexistent/path/doc.md"), "post_write");
    }

    #[test]
    fn fire_doc_event_noop_when_hooks_empty() {
        let tmp =
            std::env::temp_dir().join(format!("agent-doc-event-test-{}.md", std::process::id()));
        std::fs::write(&tmp, "---\nsession: abc\n---\nBody\n").unwrap();
        // No hooks in frontmatter — must not panic
        fire_doc_event(&tmp, "post_write");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn post_write_includes_capture_metadata_when_available() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "---\nsession: sid\n---\n\n## User\n\nHello\n").unwrap();
        crate::snapshot::save(&doc, &std::fs::read_to_string(&doc).unwrap()).unwrap();
        crate::capture::capture_response(&doc, "response").unwrap();

        fire_post_write(&doc, "sid", 1);
        let events = poll(&doc, "post_write", 0);
        assert!(!events.is_empty());
        let data = &events[0].event.data;
        assert_eq!(data["patches"].as_u64(), Some(1));
        assert!(data["capture_id"].is_string());
        assert!(data["response_sha256"].is_string());
    }

    #[test]
    fn agent_doc_prompt_target_uses_re_heading_without_model_suffix() {
        let response = "<!-- patch:exchange -->\n### Re: do [#tsiftmemhooks] — gpt-5\nDone.\n<!-- /patch:exchange -->\n";
        assert_eq!(
            extract_agent_doc_prompt_target(response).as_deref(),
            Some("do [#tsiftmemhooks]")
        );
    }

    #[test]
    fn agent_doc_response_summary_removes_patch_markers_and_truncates() {
        let body = format!(
            "<!-- patch:exchange -->\n### Re: x\n{}\n<!-- /patch:exchange -->\n",
            "a".repeat(4100)
        );
        let summary = summarize_agent_doc_response(&body);
        assert!(!summary.contains("patch:exchange"));
        assert!(summary.contains("[truncated]"));
        assert!(summary.starts_with("### Re: x"));
    }
}

//! Hook registry and document-hook I/O adapters for agent-doc.

use std::path::Path;

use agent_kit::hooks::{Event, HookRegistry};

/// Capture data attached to post-response hook events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostResponseCapture {
    pub capture_id: String,
    pub response_sha256: String,
    pub response_body: String,
}

/// Effect boundary for post-response hook metadata and closeout side effects.
pub trait PostResponseHookEffects {
    fn load_active_capture(&self, file: &Path) -> Result<Option<PostResponseCapture>, String>;
    fn capture_tsift_memory_closeout(&self, file: &Path, response_body: &str);
    fn reap_local_model_leases(&self, file: &Path);
}

/// Function-backed post-response effect adapter for command crates.
pub struct PostResponseHookEffectFns<Load, Memory, Lease> {
    load_active_capture: Load,
    capture_tsift_memory_closeout: Memory,
    reap_local_model_leases: Lease,
}

impl<Load, Memory, Lease> PostResponseHookEffectFns<Load, Memory, Lease> {
    pub fn new(
        load_active_capture: Load,
        capture_tsift_memory_closeout: Memory,
        reap_local_model_leases: Lease,
    ) -> Self {
        Self {
            load_active_capture,
            capture_tsift_memory_closeout,
            reap_local_model_leases,
        }
    }
}

impl<Load, Memory, Lease> PostResponseHookEffects for PostResponseHookEffectFns<Load, Memory, Lease>
where
    Load: Fn(&Path) -> Result<Option<PostResponseCapture>, String>,
    Memory: Fn(&Path, &str),
    Lease: Fn(&Path),
{
    fn load_active_capture(&self, file: &Path) -> Result<Option<PostResponseCapture>, String> {
        (self.load_active_capture)(file)
    }

    fn capture_tsift_memory_closeout(&self, file: &Path, response_body: &str) {
        (self.capture_tsift_memory_closeout)(file, response_body);
    }

    fn reap_local_model_leases(&self, file: &Path) {
        (self.reap_local_model_leases)(file);
    }
}

pub fn post_response_hook_effects<Load, Memory, Lease>(
    load_active_capture: Load,
    capture_tsift_memory_closeout: Memory,
    reap_local_model_leases: Lease,
) -> PostResponseHookEffectFns<Load, Memory, Lease> {
    PostResponseHookEffectFns::new(
        load_active_capture,
        capture_tsift_memory_closeout,
        reap_local_model_leases,
    )
}

fn load_active_capture_for_hooks(file: &Path) -> Result<Option<PostResponseCapture>, String> {
    if let Some(capture) = load_projected_active_capture_for_hooks(file)? {
        return Ok(Some(capture));
    }
    agent_doc_capture_io::load_active(file)
        .map(|capture| {
            capture.map(|capture| PostResponseCapture {
                capture_id: capture.capture_id,
                response_sha256: capture.response_sha256,
                response_body: capture.response_body,
            })
        })
        .map_err(|err| err.to_string())
}

fn load_projected_active_capture_for_hooks(
    file: &Path,
) -> Result<Option<PostResponseCapture>, String> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)
        .map_err(|err| err.to_string())?
    else {
        return Ok(None);
    };
    let Some(capture_id) = state.capture_id.as_deref() else {
        return Ok(None);
    };
    let Some(projected) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)
            .map_err(|err| err.to_string())?
    else {
        return Ok(None);
    };
    if projected.cycle_id != state.cycle_id
        || state
            .response_sha256
            .as_deref()
            .is_some_and(|sha| sha != projected.response_sha256)
    {
        return Ok(None);
    }
    Ok(Some(PostResponseCapture {
        capture_id: projected.capture_id,
        response_sha256: projected.response_sha256,
        response_body: projected.response_body,
    }))
}

fn capture_tsift_memory_closeout_for_hooks(file: &Path, response_body: &str) {
    let _ = agent_doc_memory_io::closeout::capture_tsift_memory_closeout(file, response_body);
}

fn reap_local_model_leases_for_hooks(file: &Path) {
    let _ = agent_doc_lease_io::local_model::reap_local_model_leases(file);
}

pub fn default_post_response_hook_effects() -> impl PostResponseHookEffects {
    post_response_hook_effects(
        load_active_capture_for_hooks,
        capture_tsift_memory_closeout_for_hooks,
        reap_local_model_leases_for_hooks,
    )
}

/// Execute document-level hooks for the given event.
///
/// Template vars `{{session_id}}`, `{{file}}`, `{{agent}}`, `{{model}}` are
/// substituted before each command is passed to `sh -c`. Best-effort: failures
/// log to stderr only.
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

        eprintln!("[hooks] {event} running: {cmd}");
        match std::process::Command::new("sh").args(["-c", &cmd]).output() {
            Ok(output) if output.status.success() => {
                eprintln!("[hooks] {event} ok");
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
                eprintln!("[hooks] {event} failed to spawn: {e}");
            }
        }
    }
}

/// Read frontmatter from `file` and fire document-level hooks for `event`.
///
/// Best-effort: if frontmatter cannot be read or hooks are empty, silently
/// returns.
pub fn fire_doc_event(file: &Path, event: &str) {
    fire_doc_event_with_authority(file, event, false);
}

/// Read frontmatter from `file` and fire document-level hooks for `event`,
/// using disk as the explicit authority when the caller is already in a
/// force-disk closeout path.
pub fn fire_doc_event_with_authority(file: &Path, event: &str, force_disk: bool) {
    let content_result = if force_disk {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(
            file,
            "hooks_fire_doc_event_force_disk",
        )
    } else {
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "hooks_fire_doc_event",
        )
    };
    let content = match content_result {
        Ok(c) => c,
        Err(_) => return,
    };
    let (fm, _) = match agent_doc_frontmatter::frontmatter::parse(&content) {
        Ok(r) => r,
        Err(_) => return,
    };
    if fm.hooks.is_empty() {
        return;
    }
    let session_id = fm.session.as_deref().unwrap_or("").to_string();
    let harness = agent_doc_model_tier::detect_harness();
    let model_config = agent_doc_model_tier::ModelConfig::default();
    let resolved_model = fm
        .resolve_harness_model(&harness)
        .map(|s| agent_doc_model_tier::canonical_model_name(s, &harness, &model_config));
    fire_doc_hooks(
        &fm.hooks,
        event,
        file,
        &session_id,
        &fm.agent,
        &resolved_model,
    );
}

/// Fire a post_write hook event using capture metadata from the effect port.
pub fn fire_post_write_with_effects(
    effects: &impl PostResponseHookEffects,
    file: &Path,
    session_id: &str,
    patch_count: usize,
) {
    let capture_metadata = effects
        .load_active_capture(file)
        .ok()
        .flatten()
        .map(|capture| capture_event_metadata(&capture));
    fire_post_write(file, session_id, patch_count, capture_metadata);
}

/// Fire a post_write hook event.
pub fn fire_post_write(
    file: &Path,
    session_id: &str,
    patch_count: usize,
    capture_metadata: Option<serde_json::Map<String, serde_json::Value>>,
) {
    if let Some(registry) = registry_for_file(file) {
        let mut data = serde_json::json!({"patches": patch_count});
        if let Some(meta) = capture_metadata
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
            .map_err(|e| eprintln!("[hooks] post_write fire failed: {e}"));
    }
}

/// Fire a post_commit hook event and run post-commit closeout effects.
pub fn fire_post_commit_with_effects(
    effects: &impl PostResponseHookEffects,
    file: &Path,
    session_id: &str,
) {
    let capture = effects.load_active_capture(file);
    let capture_metadata = capture
        .as_ref()
        .ok()
        .and_then(|capture| capture.as_ref())
        .map(capture_event_metadata);

    fire_post_commit(file, session_id, capture_metadata);

    match capture {
        Ok(Some(capture)) => effects.capture_tsift_memory_closeout(file, &capture.response_body),
        Ok(None) => {}
        Err(err) => eprintln!("[hooks] tsift-memory closeout capture skipped: {err}"),
    }
    effects.reap_local_model_leases(file);
}

/// Fire a post_commit hook event.
pub fn fire_post_commit(
    file: &Path,
    session_id: &str,
    capture_metadata: Option<serde_json::Map<String, serde_json::Value>>,
) {
    if let Some(registry) = registry_for_file(file) {
        let data = capture_metadata
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
            .map_err(|e| eprintln!("[hooks] post_commit fire failed: {e}"));
    }
}

fn capture_event_metadata(
    capture: &PostResponseCapture,
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "capture_id".to_string(),
        serde_json::Value::String(capture.capture_id.clone()),
    );
    map.insert(
        "response_sha256".to_string(),
        serde_json::Value::String(capture.response_sha256.clone()),
    );
    map
}

/// Fire a claim hook event.
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
            .map_err(|e| eprintln!("[hooks] claim fire failed: {e}"));
    }
}

/// Fire a layout_change hook event.
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
            .map_err(|e| eprintln!("[hooks] layout_change fire failed: {e}"));
    }
}

/// Poll for new events on a named hook since the given timestamp.
pub fn poll(file: &Path, hook_name: &str, since_secs: u64) -> Vec<agent_kit::hooks::ReceivedEvent> {
    registry_for_file(file)
        .and_then(|r| r.poll(hook_name, since_secs).ok())
        .unwrap_or_default()
}

fn registry_for_file(file: &Path) -> Option<HookRegistry> {
    agent_kit::hooks::hooks_dir_for_file(file).map(HookRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    struct FakePostResponseEffects {
        capture: RefCell<Result<Option<PostResponseCapture>, String>>,
        memory_closeouts: Cell<usize>,
        lease_reaps: Cell<usize>,
    }

    impl Default for FakePostResponseEffects {
        fn default() -> Self {
            Self {
                capture: RefCell::new(Ok(None)),
                memory_closeouts: Cell::new(0),
                lease_reaps: Cell::new(0),
            }
        }
    }

    impl PostResponseHookEffects for FakePostResponseEffects {
        fn load_active_capture(&self, _file: &Path) -> Result<Option<PostResponseCapture>, String> {
            self.capture.borrow().clone()
        }

        fn capture_tsift_memory_closeout(&self, _file: &Path, _response_body: &str) {
            self.memory_closeouts.set(self.memory_closeouts.get() + 1);
        }

        fn reap_local_model_leases(&self, _file: &Path) {
            self.lease_reaps.set(self.lease_reaps.get() + 1);
        }
    }

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
        assert!(output.contains("sid-1"), "session_id missing: {output}");
        assert!(output.contains("/my/doc.md"), "file missing: {output}");
        assert!(output.contains("claude"), "agent missing: {output}");
        assert!(output.contains("opus"), "model missing: {output}");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn fire_doc_hooks_noop_for_unknown_event() {
        let hooks: HashMap<String, Vec<String>> = HashMap::new();
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
        fire_doc_event(Path::new("/nonexistent/path/doc.md"), "post_write");
    }

    #[test]
    fn fire_doc_event_noop_when_hooks_empty() {
        let tmp =
            std::env::temp_dir().join(format!("agent-doc-event-test-{}.md", std::process::id()));
        std::fs::write(&tmp, "---\nsession: abc\n---\nBody\n").unwrap();
        fire_doc_event(&tmp, "post_write");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn post_write_with_effects_includes_capture_metadata_when_available() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "---\nsession: sid\n---\n\n## User\n\nHello\n").unwrap();
        let effects = FakePostResponseEffects {
            capture: RefCell::new(Ok(Some(PostResponseCapture {
                capture_id: "cap-1".to_string(),
                response_sha256: "sha-1".to_string(),
                response_body: "response".to_string(),
            }))),
            ..Default::default()
        };

        fire_post_write_with_effects(&effects, &doc, "sid", 2);
        let events = poll(&doc, "post_write", 0);
        assert!(!events.is_empty());
        let data = &events[0].event.data;
        assert_eq!(data["patches"].as_u64(), Some(2));
        assert_eq!(data["capture_id"].as_str(), Some("cap-1"));
        assert_eq!(data["response_sha256"].as_str(), Some("sha-1"));
    }

    #[test]
    fn load_active_capture_for_hooks_uses_projection_without_capture_sidecar() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        let base = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        let response = "### Re: hello — gpt-5\n\nDone.\n";
        std::fs::write(&doc, base).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        assert!(!capture.capture_id.is_empty());

        let projected = load_active_capture_for_hooks(&doc)
            .unwrap()
            .expect("projected active capture");
        assert_eq!(projected.capture_id, capture.capture_id);
        assert_eq!(projected.response_sha256, capture.response_sha256);
        assert_eq!(projected.response_body, response);
    }

    #[test]
    fn post_commit_with_effects_fires_event_and_runs_closeouts() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "---\nsession: sid\n---\n\n## User\n\nHello\n").unwrap();
        let effects = FakePostResponseEffects {
            capture: RefCell::new(Ok(Some(PostResponseCapture {
                capture_id: "cap-2".to_string(),
                response_sha256: "sha-2".to_string(),
                response_body: "response".to_string(),
            }))),
            ..Default::default()
        };

        fire_post_commit_with_effects(&effects, &doc, "sid");
        let events = poll(&doc, "post_commit", 0);
        let event = events
            .iter()
            .find(|event| event.event.data["capture_id"].as_str() == Some("cap-2"))
            .expect("post_commit event should include capture metadata");
        let data = &event.event.data;
        assert_eq!(data["capture_id"].as_str(), Some("cap-2"));
        assert_eq!(data["response_sha256"].as_str(), Some("sha-2"));
        assert_eq!(effects.memory_closeouts.get(), 1);
        assert_eq!(effects.lease_reaps.get(), 1);
    }
}

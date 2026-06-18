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
    let _ = reap_local_model_leases(file);
    reap_stale_jetbrains_consumers_closeout(file);
}

/// `#fccreap`: closeout wrapper that resolves the project root from the session
/// document and reaps stale dead-PID IntelliJ plugin consumer patch files.
/// Best-effort: a missing project root is a silent no-op, and a reap of zero
/// files logs nothing. Logs a one-line ops summary only when it reaps > 0.
fn reap_stale_jetbrains_consumers_closeout(file: &Path) {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return;
    };
    let reaped = reap_stale_jetbrains_consumers(&project_root);
    if reaped > 0 {
        eprintln!("[hooks] reaped {reaped} stale jetbrains consumer patch file(s)");
    }
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

/// Relative default cooperative GPU lease registry path (matches
/// tsift-local-model's `resolve_lease_file` default). Used as the project-root
/// guard so the closeout reap only fires for projects that actually use tsift
/// model leasing.
const DEFAULT_LEASE_REGISTRY_RELATIVE: &str = ".tsift/gpu-lease.json";

/// Outcome of one best-effort closeout lease reap. Returned for inspectability
/// and unit testing; the runtime closeout path ignores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReapOutcome {
    /// Reap ran and exited 0. Carries stdout (JSON) for diagnostics.
    Reaped(String),
    /// This file has no tsift project root — nothing to reap.
    SkippedNoProjectRoot,
    /// No `.tsift/gpu-lease.json` — tsift model leasing not in use here.
    SkippedNoRegistry,
    /// `tsift` binary could not be spawned (e.g. not on PATH).
    SpawnFailed(String),
    /// Reap exited non-zero. Carries stderr.
    NonZeroExit(Option<i32>, String),
}

/// Build the `tsift local-model lease reap` arg vector. Pure and unit-tested;
/// `build_reap_command` consumes it.
fn reap_command_args(lease_file: Option<&str>, host: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "local-model".to_string(),
        "lease".to_string(),
        "reap".to_string(),
        "--unload-empty".to_string(),
    ];
    if let Some(path) = lease_file {
        args.push("--lease-file".to_string());
        args.push(path.to_string());
    }
    if let Some(host) = host {
        args.push("--host".to_string());
        args.push(host.to_string());
    }
    args
}

/// #kgleasereap: best-effort automatic reclamation of crashed-session GPU
/// leases from the closeout (post-commit) hook.
///
/// Runs `tsift local-model lease reap --unload-empty` under the resolved
/// project root when a lease registry is present, so pid-dead holders left by
/// crashed `tsift kg extract` runs are reclaimed and the now-unreferenced model
/// is unloaded (Ollama `keep_alive:0`) without a manual CLI invocation. Mirrors
/// [`capture_tsift_memory_closeout`]: spawn failures, non-zero exits, and a
/// missing registry degrade to a logged warning — closeout must never fail
/// because a lease reap could not run. Reap is safe during an active cycle:
/// `#kgreflease` only reclaims pid-dead or TTL-expired holders, never a
/// concurrent live extractor's lease.
pub(crate) fn reap_local_model_leases(file: &Path) -> ReapOutcome {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = crate::snapshot::find_project_root(&canonical) else {
        return ReapOutcome::SkippedNoProjectRoot;
    };
    if !project_root
        .join(DEFAULT_LEASE_REGISTRY_RELATIVE)
        .exists()
    {
        return ReapOutcome::SkippedNoRegistry;
    }
    let args = reap_command_args(None, None);
    let mut cmd = std::process::Command::new("tsift");
    cmd.args(&args).current_dir(&project_root).arg("--json");
    match cmd.output() {
        Ok(output) if output.status.success() => {
            eprintln!(
                "[hooks] tsift lease reap ok for {}",
                file.display()
            );
            ReapOutcome::Reaped(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(output) => {
            eprintln!(
                "[hooks] tsift lease reap exited with code {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            ReapOutcome::NonZeroExit(
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            )
        }
        Err(err) => {
            eprintln!("[hooks] tsift lease reap failed to spawn: {err}");
            ReapOutcome::SpawnFailed(err.to_string())
        }
    }
}

/// `#fccreap`: parse the IntelliJ-plugin consumer pid out of a per-instance
/// patch filename.
///
/// The JetBrains plugin registers a per-instance consumer id
/// `jetbrains-<pid>-<uuid>` (see
/// `editors/jetbrains/.../TypingTracker.kt`), and per-instance patch files land
/// in `.agent-doc/patches/` named `<doc_hash>.jetbrains-<pid>-<uuid>.json`.
///
/// Returns `Some(pid)` only for filenames that contain the literal `.jetbrains-`
/// marker followed by a run of ASCII digits (the pid). Returns `None` for the
/// base `<hash>.json`, `.vscode` variants, or any name where the pid cannot be
/// parsed — callers must treat `None` as "do not reap".
fn jetbrains_consumer_pid(filename: &str) -> Option<u32> {
    if !filename.ends_with(".json") {
        return None;
    }
    // The marker is `.jetbrains-` so the base `<hash>.json` (no per-instance
    // suffix) and `.vscode` variants never match.
    let after_marker = filename.split(".jetbrains-").nth(1)?;
    let digits: String = after_marker
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    // The pid must be followed by `-<uuid>` (a hyphen), never the file extension
    // or end-of-string — otherwise the name is malformed.
    let rest = &after_marker[digits.len()..];
    if !rest.starts_with('-') {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// `#fccreap`: best-effort reap of stale dead-PID IntelliJ plugin consumer patch
/// files from `<project_root>/.agent-doc/patches/`.
///
/// When an IntelliJ instance dies/restarts (or multiple windows are open), its
/// per-instance `<hash>.jetbrains-<pid>-<uuid>.json` patch files accumulate and
/// are never reaped, bloating the patches dir and contributing to multi-instance
/// IPC confusion. This removes only files whose pid is provably dead.
///
/// Best-effort: directory/read/remove errors degrade to a logged stderr warning;
/// closeout must never fail because a reap could not run. On non-Unix this is a
/// no-op (returns 0). Returns the number of files reaped.
pub fn reap_stale_jetbrains_consumers(project_root: &Path) -> usize {
    let patches_dir = project_root.join(".agent-doc").join("patches");
    reap_stale_jetbrains_consumers_with(&patches_dir, pid_is_live)
}

/// `#fccreap`: testable core of [`reap_stale_jetbrains_consumers`]. Takes an
/// injectable liveness predicate so unit tests can avoid real processes.
///
/// Only files matching `*.jetbrains-<digits>-<uuid>.json` whose pid is NOT live
/// (and is not this process's own pid) are removed. Non-matching names (base
/// `<hash>.json`, `.vscode` variants, unrelated files) and unparseable pids are
/// always skipped.
fn reap_stale_jetbrains_consumers_with(
    patches_dir: &Path,
    is_pid_live: impl Fn(u32) -> bool,
) -> usize {
    let entries = match std::fs::read_dir(patches_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(err) => {
            eprintln!(
                "[hooks] jetbrains consumer reap: cannot read {}: {err}",
                patches_dir.display()
            );
            return 0;
        }
    };
    let self_pid = std::process::id();
    let mut reaped = 0usize;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                eprintln!("[hooks] jetbrains consumer reap: dir entry error: {err}");
                continue;
            }
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        // Conservative: only consider files whose pid we can parse; never reap a
        // non-`jetbrains-` patch file or this process's own pid.
        let Some(pid) = jetbrains_consumer_pid(name) else {
            continue;
        };
        if pid == self_pid || is_pid_live(pid) {
            continue;
        }
        let path = entry.path();
        match std::fs::remove_file(&path) {
            Ok(()) => reaped += 1,
            Err(err) => {
                eprintln!(
                    "[hooks] jetbrains consumer reap: failed to remove {}: {err}",
                    path.display()
                );
            }
        }
    }
    reaped
}

/// `#fccreap`: real liveness check used by [`reap_stale_jetbrains_consumers`].
///
/// On Unix, `kill(pid, 0)` returning 0 (process exists) or erroring with `EPERM`
/// (process exists, not permitted) both mean ALIVE; only `ESRCH` (no such
/// process) means dead. On non-Unix this conservatively reports every pid as
/// live so nothing is reaped.
#[cfg(unix)]
fn pid_is_live(pid: u32) -> bool {
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_is_live(_pid: u32) -> bool {
    true
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

    #[test]
    fn reap_command_args_defaults_to_unload_empty_without_optional_flags() {
        let args = reap_command_args(None, None);
        assert_eq!(
            args,
            vec![
                "local-model".to_string(),
                "lease".to_string(),
                "reap".to_string(),
                "--unload-empty".to_string(),
            ]
        );
    }

    #[test]
    fn reap_command_args_appends_lease_file_and_host_when_given() {
        let args = reap_command_args(Some(".tsift/gpu-lease.json"), Some("http://gpu-box:11434"));
        assert!(args.contains(&"--lease-file".to_string()));
        assert!(args.contains(&".tsift/gpu-lease.json".to_string()));
        assert!(args.contains(&"--host".to_string()));
        assert!(args.contains(&"http://gpu-box:11434".to_string()));
        // Order: --unload-empty precedes the optional overrides.
        let unload_idx = args
            .iter()
            .position(|a| a == "--unload-empty")
            .unwrap();
        let host_idx = args.iter().position(|a| a == "--host").unwrap();
        assert!(unload_idx < host_idx);
    }

    #[test]
    fn jetbrains_consumer_pid_parses_pid_and_rejects_non_matching() {
        assert_eq!(
            jetbrains_consumer_pid("abc123.jetbrains-12345-1f2e3d4c-uuid.json"),
            Some(12345)
        );
        // Base patch file (no per-instance suffix) → None.
        assert_eq!(jetbrains_consumer_pid("abc123.json"), None);
        // VS Code variant → None.
        assert_eq!(jetbrains_consumer_pid("abc123.vscode-99-uuid.json"), None);
        // Malformed: pid not followed by `-<uuid>`.
        assert_eq!(jetbrains_consumer_pid("abc123.jetbrains-12345.json"), None);
        // Malformed: no digits after the marker.
        assert_eq!(jetbrains_consumer_pid("abc123.jetbrains--uuid.json"), None);
        // Not a json file.
        assert_eq!(jetbrains_consumer_pid("abc123.jetbrains-12345-uuid.txt"), None);
    }

    #[test]
    fn reap_removes_only_dead_pid_consumer_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let patches = tmp.path().join("patches");
        std::fs::create_dir_all(&patches).unwrap();

        // Dead-pid jetbrains consumer files (should be reaped).
        let dead_a = patches.join("h1.jetbrains-111-aaaa.json");
        let dead_b = patches.join("h2.jetbrains-222-bbbb.json");
        // Alive-pid jetbrains consumer file (should survive).
        let alive = patches.join("h3.jetbrains-333-cccc.json");
        // Base patch file + unrelated file (should survive).
        let base = patches.join("h4.json");
        let unrelated = patches.join("notes.txt");
        for p in [&dead_a, &dead_b, &alive, &base, &unrelated] {
            std::fs::write(p, "{}").unwrap();
        }

        // Fake predicate: only pid 333 is "alive".
        let reaped = reap_stale_jetbrains_consumers_with(&patches, |pid| pid == 333);

        assert_eq!(reaped, 2);
        assert!(!dead_a.exists(), "dead-pid file A should be reaped");
        assert!(!dead_b.exists(), "dead-pid file B should be reaped");
        assert!(alive.exists(), "alive-pid file should survive");
        assert!(base.exists(), "base patch file should survive");
        assert!(unrelated.exists(), "unrelated file should survive");
    }

    #[test]
    fn reap_is_noop_on_empty_or_missing_dir() {
        // Missing dir.
        let tmp = tempfile::TempDir::new().unwrap();
        let missing = tmp.path().join("patches");
        assert_eq!(
            reap_stale_jetbrains_consumers_with(&missing, |_| false),
            0
        );

        // Empty dir.
        std::fs::create_dir_all(&missing).unwrap();
        assert_eq!(
            reap_stale_jetbrains_consumers_with(&missing, |_| false),
            0
        );
    }

    #[test]
    fn reap_skips_without_project_root_and_never_spawns() {
        // A path with no agent-doc project-root marker resolves to no project
        // root, so the reap returns early without spawning `tsift`.
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = tmp.path().join("orphan.md");
        std::fs::write(&doc, "no frontmatter").unwrap();
        let outcome = reap_local_model_leases(&doc);
        assert!(
            matches!(outcome, ReapOutcome::SkippedNoProjectRoot | ReapOutcome::SkippedNoRegistry),
            "expected a skip outcome, got {outcome:?}"
        );
    }
}

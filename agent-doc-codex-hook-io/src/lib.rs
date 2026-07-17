//! Codex hook state-ledger persistence.

use agent_doc_model_tier::context_transcript_io::{
    latest_codex_transcript, transcript_context_pct,
};
use agent_doc_model_tier::context_usage::{Harness, clear_decision};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Resolve the nearest agent-doc project root for `path`.
pub fn project_root_for(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    agent_doc_project_root_io::project_root_containing(&canonical)
}

/// Roots that should receive Codex hook state for `path`.
///
/// The nearest `.agent-doc` root is always first. When the path lives inside a
/// git worktree whose root also has `.agent-doc`, that git root is included as a
/// second state location so hooks remain visible from both nested and workspace
/// scopes.
pub fn project_roots_for(path: &Path) -> Vec<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Some(nearest_root) = project_root_for(&canonical) else {
        return Vec::new();
    };
    let mut roots = vec![nearest_root.clone()];
    let Some(git_root) = find_git_root(&canonical) else {
        return roots;
    };
    if !nearest_root.starts_with(&git_root) {
        return roots;
    }
    if git_root.join(".agent-doc").is_dir() {
        push_unique_root(&mut roots, git_root);
    }
    roots
}

/// Roots that should be consulted or updated for a hook payload with a cwd and
/// optional document target.
pub fn tracking_roots(cwd: &Path, doc_path: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = project_roots_for(cwd);
    if let Some(doc_path) = doc_path {
        for root in project_roots_for(doc_path) {
            push_unique_root(&mut roots, root);
        }
    }
    roots
}

/// Append `root` once, preserving first-seen order.
pub fn push_unique_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !roots.iter().any(|existing| existing == &root) {
        roots.push(root);
    }
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };
    while let Some(path) = current {
        if path.join(".git").exists() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    pub session_id: String,
    pub doc_path: String,
    pub last_turn_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_auto_queue_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_context_clear_at: Option<u64>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSessionState {
    pub session_id: String,
    pub doc_path: String,
    pub last_turn_id: String,
    pub last_prompt: String,
    pub updated_at: u64,
}

const CODEX_HOOK_SESSION_PREFIX: &str = "codex_hook_session:";

fn session_state_key(session_id: &str) -> String {
    format!(
        "{CODEX_HOOK_SESSION_PREFIX}{}",
        agent_doc_hash::content_hash(session_id)
    )
}

#[derive(Debug, Deserialize)]
pub struct UserPromptSubmitInput {
    pub session_id: String,
    pub turn_id: String,
    pub cwd: String,
    pub prompt: String,
}

pub fn handle_user_prompt_submit() -> Result<()> {
    let payload = match read_stdin_payload() {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!("[codex-hook] user-prompt-submit payload read failed: {err}");
            return Ok(());
        }
    };
    let input: UserPromptSubmitInput = match serde_json::from_str(&payload) {
        Ok(input) => input,
        Err(err) => {
            eprintln!("[codex-hook] user-prompt-submit JSON parse failed: {err}");
            return Ok(());
        }
    };
    apply_user_prompt_submit(&input)
}

pub fn apply_user_prompt_submit(input: &UserPromptSubmitInput) -> Result<()> {
    let cwd = PathBuf::from(&input.cwd);
    let roots_for_cwd = project_roots_for(&cwd);
    let previous_state = load_state_any(&roots_for_cwd, &input.session_id)?.map(|(_, state)| state);
    let doc_path = resolve_agent_doc_path(&input.prompt, &cwd).or_else(|| {
        previous_state
            .as_ref()
            .map(|state| PathBuf::from(&state.doc_path))
    });
    let Some(doc_path) = doc_path else {
        return Ok(());
    };
    let roots = tracking_roots(&cwd, Some(&doc_path));
    if roots.is_empty() {
        return Ok(());
    }

    let now = now_secs();
    let last_context_clear_at = if prompt_requests_clear(&input.prompt) {
        Some(now)
    } else {
        previous_state
            .as_ref()
            .and_then(|state| state.last_context_clear_at)
    };
    let state = SessionState {
        session_id: input.session_id.clone(),
        doc_path: doc_path.display().to_string(),
        last_turn_id: input.turn_id.clone(),
        last_prompt: input.prompt.clone(),
        last_auto_queue_head: None,
        last_context_clear_at,
        updated_at: now,
    };
    for root in roots {
        save_state(&root, &state)?;
    }
    Ok(())
}

pub fn resolve_agent_doc_path(prompt: &str, cwd: &Path) -> Option<PathBuf> {
    let file =
        agent_doc_prompt_contract::harness_prompt::agent_doc_invocation_file_from_text(prompt)?;
    let path = PathBuf::from(file);
    let joined = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    Some(joined.canonicalize().unwrap_or(joined))
}

pub fn load_state_any(
    roots: &[PathBuf],
    session_id: &str,
) -> Result<Option<(PathBuf, SessionState)>> {
    for root in roots {
        if let Some(state) = load_state(root, session_id)? {
            return Ok(Some((root.clone(), state)));
        }
    }
    Ok(None)
}

pub fn clear_state_across_roots(
    roots: &[PathBuf],
    loaded_root: &Path,
    session_id: &str,
) -> Result<()> {
    let mut all_roots = roots.to_vec();
    push_unique_root(&mut all_roots, loaded_root.to_path_buf());
    for root in all_roots {
        clear_state(&root, session_id)?;
    }
    Ok(())
}

pub fn save_state_across_roots(
    roots: &[PathBuf],
    loaded_root: &Path,
    state: &SessionState,
) -> Result<()> {
    let mut all_roots = roots.to_vec();
    push_unique_root(&mut all_roots, loaded_root.to_path_buf());
    for root in all_roots {
        save_state(&root, state)?;
    }
    Ok(())
}

pub fn load_state(root: &Path, session_id: &str) -> Result<Option<SessionState>> {
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &conn,
        &session_state_key(session_id),
    )?
    .map(|content| serde_json::from_str(&content).context("parse Codex hook session state"))
    .transpose()
}

pub fn load_prompt_for_current_session(file: &Path) -> Result<Option<String>> {
    let Some(state) = load_active_session_for_current_file(file)? else {
        return Ok(None);
    };
    if state.last_prompt.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(state.last_prompt))
}

pub fn load_latest_prompt_for_file(file: &Path) -> Result<Option<String>> {
    let Some(state) = load_latest_state_for_file(file)? else {
        return Ok(None);
    };
    if state.last_prompt.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(state.last_prompt))
}

pub fn load_latest_prompt_state_for_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    load_latest_state_for_file(file)
}

pub fn prompt_requests_clear(prompt: &str) -> bool {
    matches!(prompt.trim(), "/clear" | "/new")
}

pub fn agent_doc_mcp_configured_for(file: &Path) -> bool {
    project_roots_for(file).iter().any(|root| {
        let config_path = root.join(".codex/config.toml");
        let Ok(content) = std::fs::read_to_string(&config_path) else {
            return false;
        };
        let Ok(config) = toml::from_str::<toml::Value>(&content) else {
            return false;
        };
        config
            .get("mcp_servers")
            .and_then(toml::Value::as_table)
            .and_then(|servers| servers.get("agent-doc"))
            .and_then(toml::Value::as_table)
            .map(|server| {
                server.get("command").and_then(toml::Value::as_str) == Some("agent-doc")
                    || server.get("url").and_then(toml::Value::as_str).is_some()
            })
            .unwrap_or(false)
    })
}

pub fn is_context_clear_prompt(prompt: &str) -> bool {
    agent_doc_queue::queue_command::is_context_clear_command(prompt)
}

/// `#clearcodex`: resolve the Codex Stop-hook continuation context-reset reason
/// and emit the structured proof lines an operator greps for in ops.log.
pub fn codex_live_context_pct(file: &Path) -> Option<f64> {
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    let project_dir = project_roots_for(file)
        .into_iter()
        .next()
        .or_else(|| std::env::current_dir().ok())?;
    let transcript = latest_codex_transcript(Path::new(&home), &project_dir)?;
    transcript_context_pct(Harness::Codex, &transcript, "codex")
}

pub fn codex_queue_context_reset_reason(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Result<Option<String>> {
    let mut reason = agent_doc_session_accretion_io::queue_context_reset_reason_if_opted_in(
        file,
        last_context_clear_at,
    )?;
    if agent_doc_session_accretion_io::queue_context_reset_opted_in(file) {
        let threshold = agent_doc_session_accretion_io::clear_threshold_for_doc(file);
        let pct = codex_live_context_pct(file);
        let decision = clear_decision(true, pct, threshold);
        if reason.is_none() && decision.clear {
            reason = Some(format!(
                "transcript context {:.1}% >= clear threshold {}% (#clearcodex)",
                pct.unwrap_or_default(),
                threshold
            ));
        }
    }
    Ok(reason)
}

pub fn codex_continuation_clear_reason(
    file: &Path,
    last_context_clear_at: Option<u64>,
) -> Option<String> {
    let reason = match codex_queue_context_reset_reason(file, last_context_clear_at) {
        Ok(reason) => reason,
        Err(err) => {
            eprintln!(
                "[agent-doc] codex stop hook: failed to resolve queue context-reset reason for {}: {err:#}",
                file.display()
            );
            None
        }
    };
    if agent_doc_session_accretion_io::queue_context_reset_opted_in(file) {
        let threshold = agent_doc_session_accretion_io::clear_threshold_for_doc(file);
        let pct = codex_live_context_pct(file);
        let decision = clear_decision(true, pct, threshold);
        agent_doc_ops_log_io::log_op(file, &decision.diagnostic);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "[clearcodex] codex-continuation optIn=true reason={:?} clear_instructed=false background_clear_suppressed={}",
                reason.as_deref().unwrap_or(""),
                reason.is_some()
            ),
        );
    }
    reason
}

pub fn log_codex_stop_queue_continuation(file: &Path, prompt: &str, source: &str) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_stop_queue_continuation file={} source={} mcp_configured={} prompt_bytes={} prompt_sha256={}",
            file.display(),
            source,
            agent_doc_mcp_configured_for(file),
            prompt.len(),
            agent_doc_hash::content_hash(prompt),
        ),
    );
}

pub fn log_codex_background_context_clear_suppressed(
    file: &Path,
    prompt: &str,
    source: &str,
    reason: &str,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "codex_background_context_clear_suppressed file={} source={} result=in_pane_continuation prompt_bytes={} prompt_sha256={} reason={:?}",
            file.display(),
            source,
            prompt.len(),
            agent_doc_hash::content_hash(prompt),
            reason,
        ),
    );
}

pub fn record_external_prompt_for_file(file: &Path, session_id: &str, prompt: &str) -> Result<()> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let state = SessionState {
        session_id: session_id.to_string(),
        doc_path: canonical.display().to_string(),
        last_turn_id: String::new(),
        last_prompt: prompt.to_string(),
        last_auto_queue_head: None,
        last_context_clear_at: prompt_requests_clear(prompt).then(now_secs),
        updated_at: now_secs(),
    };
    for root in project_roots_for(&canonical) {
        save_state(&root, &state)?;
    }
    Ok(())
}

pub fn load_active_session_for_current_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    let Some(session_id) = current_session_id() else {
        return Ok(None);
    };
    let roots = project_roots_for(file);
    let Some((_, state)) = load_state_any(&roots, &session_id)? else {
        return Ok(None);
    };
    let state_file = PathBuf::from(&state.doc_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&state.doc_path));
    let current_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if state_file != current_file {
        return Ok(None);
    }
    Ok(Some(active_session_state(state)))
}

pub fn save_state(root: &Path, state: &SessionState) -> Result<()> {
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &session_state_key(&state.session_id),
        &serde_json::to_string(state)?,
        state.updated_at.saturating_mul(1000),
    )
}

pub fn clear_state(root: &Path, session_id: &str) -> Result<()> {
    let conn = agent_doc_sqlite::state_store::open_state_db(root)?;
    agent_doc_sqlite::state_store::clear_project_runtime_state_in_db(
        &conn,
        &session_state_key(session_id),
    )
}

fn read_stdin_payload() -> Result<String> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .context("read hook payload from stdin")?;
    Ok(payload)
}

pub fn current_session_id() -> Option<String> {
    std::env::var("CODEX_THREAD_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("CODEX_SESSION_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn load_latest_state_for_file(file: &Path) -> Result<Option<ActiveSessionState>> {
    let current_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let mut latest: Option<SessionState> = None;

    for root in project_roots_for(file) {
        let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
        for (_, content, _) in agent_doc_sqlite::state_store::list_project_runtime_state_from_db(
            &conn,
            CODEX_HOOK_SESSION_PREFIX,
        )? {
            let Ok(state) = serde_json::from_str::<SessionState>(&content) else {
                continue;
            };
            let state_file = PathBuf::from(&state.doc_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&state.doc_path));
            if state_file != current_file {
                continue;
            }
            let is_newer = latest
                .as_ref()
                .is_none_or(|best| state.updated_at >= best.updated_at);
            if is_newer {
                latest = Some(state);
            }
        }
    }

    Ok(latest.map(active_session_state))
}

fn active_session_state(state: SessionState) -> ActiveSessionState {
    ActiveSessionState {
        session_id: state.session_id,
        doc_path: state.doc_path,
        last_turn_id: state.last_turn_id,
        last_prompt: state.last_prompt,
        updated_at: state.updated_at,
    }
}

#[derive(Debug, Serialize)]
struct BlockedStopPayloadRecord<'a> {
    captured_at: u64,
    file: String,
    kind: &'a str,
    reason: &'a str,
    payload_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_assistant_message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_prompt: Option<&'a str>,
}

pub fn save_blocked_stop_payload(
    file: &Path,
    payload: &str,
    reason: &str,
    kind: &str,
    last_prompt: Option<&str>,
) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .context("resolve project root for blocked stop payload")?;
    let dir = root.join(".agent-doc/codex-hooks/blocked-stop");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create blocked-stop dir {}", dir.display()))?;
    let filename = format!(
        "{}-{}.json",
        agent_doc_hash::content_hash(canonical.to_string_lossy().as_ref()),
        now_millis()
    );
    let path = dir.join(filename);
    let record = BlockedStopPayloadRecord {
        captured_at: now_secs(),
        file: canonical.display().to_string(),
        kind,
        reason,
        payload_sha256: agent_doc_hash::content_hash(payload),
        last_assistant_message: (!payload.trim().is_empty()).then_some(payload),
        last_prompt: last_prompt.filter(|prompt| !prompt.trim().is_empty()),
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(&path, json)
        .with_context(|| format!("write blocked stop payload {}", path.display()))?;
    Ok(path)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tempdir_without_agent_doc_ancestor() -> tempfile::TempDir {
        for base in [
            Path::new("/var/tmp"),
            Path::new("/dev/shm"),
            Path::new("/tmp"),
        ] {
            if !base.is_dir() || agent_doc_project_root_io::project_root_containing(base).is_some()
            {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-codex-hook-io-")
                .tempdir_in(base)
                && agent_doc_project_root_io::project_root_containing(dir.path()).is_none()
            {
                return dir;
            }
        }
        panic!("no writable temp base without a .agent-doc ancestor");
    }

    #[test]
    fn saves_blocked_stop_payload_under_project_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path = save_blocked_stop_payload(
            &doc,
            "assistant response",
            "component dump",
            "blocked_replay_payload",
            Some("agent-doc task.md"),
        )
        .unwrap();

        assert!(path.starts_with(root.join(".agent-doc/codex-hooks/blocked-stop")));
        let json = std::fs::read_to_string(path).unwrap();
        assert!(json.contains("\"kind\": \"blocked_replay_payload\""));
        assert!(json.contains("\"reason\": \"component dump\""));
        assert!(json.contains("\"last_assistant_message\": \"assistant response\""));
        assert!(json.contains("\"last_prompt\": \"agent-doc task.md\""));
    }

    #[test]
    fn falls_back_to_file_parent_without_project_root() {
        let dir = tempdir_without_agent_doc_ancestor();
        let doc = dir.path().join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        let path =
            save_blocked_stop_payload(&doc, "", "missing response", "missing", None).unwrap();

        assert!(path.starts_with(dir.path().join(".agent-doc/codex-hooks/blocked-stop")));
        let json = std::fs::read_to_string(path).unwrap();
        assert!(json.contains("\"kind\": \"missing\""));
        assert!(!json.contains("last_assistant_message"));
        assert!(!json.contains("last_prompt"));
    }

    #[test]
    fn project_roots_for_returns_nearest_then_git_root_when_both_have_agent_doc() {
        let dir = tempfile::tempdir().unwrap();
        let git_root = dir.path();
        std::fs::create_dir_all(git_root.join(".git")).unwrap();
        std::fs::create_dir_all(git_root.join(".agent-doc")).unwrap();
        let nested = git_root.join("nested");
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("task.md");
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert_eq!(
            project_roots_for(&doc),
            vec![nested, git_root.to_path_buf()]
        );
    }

    #[test]
    fn tracking_roots_merges_cwd_and_document_roots_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks/task.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\n---\n").unwrap();

        assert_eq!(tracking_roots(root, Some(&doc)), vec![root.to_path_buf()]);
    }

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        dir
    }

    fn write_doc(dir: &tempfile::TempDir) -> PathBuf {
        let doc = dir.path().join("task.md");
        fs::write(&doc, "---\nsession: sid\n---\n\n## User\n\nHello\n").unwrap();
        doc
    }

    fn track_doc(dir: &tempfile::TempDir, doc: &Path, turn_id: &str) {
        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: turn_id.to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        })
        .unwrap();
    }

    #[test]
    fn user_prompt_submit_tracks_agent_doc_file() {
        let dir = setup_project();
        let doc = write_doc(&dir);

        track_doc(&dir, &doc, "turn-1");

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(PathBuf::from(state.doc_path), doc);
        assert_eq!(state.last_turn_id, "turn-1");
        assert_eq!(state.last_prompt, format!("agent-doc {}", doc.display()));
    }

    #[test]
    fn user_prompt_submit_does_not_track_ambient_ancestor_root() {
        let ambient = tempfile::tempdir().unwrap();
        fs::create_dir_all(ambient.path().join(".agent-doc")).unwrap();
        let project = ambient.path().join("project");
        fs::create_dir_all(project.join(".agent-doc")).unwrap();
        let doc = project.join("task.md");
        fs::write(&doc, "---\nsession: sid\n---\n\n## User\n\nHello\n").unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: project.display().to_string(),
            prompt: format!("agent-doc {}", doc.display()),
        })
        .unwrap();

        let project_state = load_state(&project, "codex-session").unwrap();
        let ambient_state = load_state(ambient.path(), "codex-session").unwrap();
        assert!(
            project_state.is_some(),
            "nearest project root should receive Codex hook state"
        );
        assert!(
            ambient_state.is_none(),
            "ambient ancestor .agent-doc roots must not receive shared hook state"
        );
    }

    #[test]
    fn user_prompt_submit_tracks_same_line_agent_doc_body() {
        let dir = setup_project();
        let doc = write_doc(&dir);

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!("agent-doc {} #code-review", doc.display()),
        })
        .unwrap();

        let root = project_root_for(dir.path()).unwrap();
        let state = load_state(&root, "codex-session").unwrap().unwrap();
        assert_eq!(PathBuf::from(&state.doc_path), doc);
        assert_eq!(
            state.last_prompt,
            format!("agent-doc {} #code-review", doc.display())
        );

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK
            .lock()
            .unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };
        let loaded = agent_doc_harness::prompt_source::prompt_body_for_file(
            &doc,
            load_prompt_for_current_session,
        )
        .unwrap();
        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(loaded, Some("#code-review".to_string()));
    }

    #[test]
    fn resolve_agent_doc_path_prefers_real_invocation_after_instruction_preamble() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let prompt = format!(
            "# AGENTS.md instructions for {}\n\
\n\
```\n\
agent-doc <FILE>\n\
agent-doc compact <FILE>\n\
```\n\
\n\
Use the harness-native entrypoint below.\n\
\n\
agent-doc {}\n",
            dir.path().display(),
            doc.display()
        );

        let resolved = resolve_agent_doc_path(&prompt, dir.path()).expect("doc path");

        assert_eq!(resolved, doc);
    }

    #[test]
    fn resolve_agent_doc_path_accepts_session_invocation_with_trailing_body() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let prompt = format!("agent-doc {} #agent-doc-bug", doc.display());

        let resolved = resolve_agent_doc_path(&prompt, dir.path()).expect("doc path");

        assert_eq!(resolved, doc);
    }

    #[test]
    fn load_prompt_for_current_session_uses_codex_thread_id() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        track_doc(&dir, &doc, "turn-1");

        let _lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK
            .lock()
            .unwrap();
        let prev = std::env::var("CODEX_THREAD_ID").ok();
        unsafe { std::env::set_var("CODEX_THREAD_ID", "codex-session") };
        let loaded = load_prompt_for_current_session(&doc).unwrap();
        if let Some(value) = prev {
            unsafe { std::env::set_var("CODEX_THREAD_ID", value) };
        } else {
            unsafe { std::env::remove_var("CODEX_THREAD_ID") };
        }

        assert_eq!(loaded, Some(format!("agent-doc {}", doc.display())));
    }

    #[test]
    fn load_latest_prompt_for_file_picks_most_recent_matching_state() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let root = project_root_for(dir.path()).unwrap();

        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-old".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: format!("agent-doc {}", doc.display()),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                updated_at: 10,
            },
        )
        .unwrap();
        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-new".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-2".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(20),
                updated_at: 20,
            },
        )
        .unwrap();

        let loaded = load_latest_prompt_for_file(&doc).unwrap();
        assert_eq!(loaded.as_deref(), Some("/clear"));
    }

    #[test]
    fn load_latest_prompt_for_file_skips_malformed_ledger_rows() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let root = project_root_for(dir.path()).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            &format!("{CODEX_HOOK_SESSION_PREFIX}bad"),
            "{",
            1,
        )
        .unwrap();

        save_state(
            &root,
            &SessionState {
                session_id: "codex-session-good".to_string(),
                doc_path: doc.display().to_string(),
                last_turn_id: "turn-1".to_string(),
                last_prompt: "/clear".to_string(),
                last_auto_queue_head: None,
                last_context_clear_at: Some(20),
                updated_at: 20,
            },
        )
        .unwrap();

        let loaded = load_latest_prompt_for_file(&doc).unwrap();
        assert_eq!(loaded.as_deref(), Some("/clear"));
    }

    #[test]
    fn prompt_requests_clear_matches_only_exact_builtin() {
        assert!(prompt_requests_clear("/clear"));
        assert!(prompt_requests_clear("  /clear  "));
        assert!(prompt_requests_clear("/new"));
        assert!(!prompt_requests_clear("agent-doc tasks/foo.md"));
        assert!(!prompt_requests_clear("/clear please"));
    }
}

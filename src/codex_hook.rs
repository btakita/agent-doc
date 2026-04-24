//! # Module: codex_hook
//!
//! ## Spec
//! - Implements the repo-local Codex hook bridge used by `agent-doc` installs.
//! - `handle_user_prompt_submit()` reads the Codex `UserPromptSubmit` JSON payload
//!   from stdin, detects `agent-doc <FILE>`-style invocations, and records the
//!   active document for the Codex session under `.agent-doc/codex-hooks/`.
//! - `handle_stop()` reads the Codex `Stop` JSON payload from stdin, checks the
//!   tracked document with `session_check::inspect()`, and only intervenes when
//!   the cycle is still open.
//! - On the first intercepted stop, the hook first tries to finish the response
//!   cycle deterministically: save/capture `last_assistant_message`, replay it
//!   through `recover`, and run the normal `git::commit()` boundary.
//! - If the cycle still cannot be closed automatically, the hook falls back to
//!   blocking the turn with instructions to finish recovery/persistence.
//! - If Codex reaches a second `Stop` for the same still-open cycle
//!   (`stop_hook_active = true`), fail closed with `continue=false` instead of
//!   looping forever.
//!
//! ## Agentic Contracts
//! - Hook handling is deterministic and binary-owned; generated Codex hook files
//!   should only shell out to these commands.
//! - Missing project roots, unmatched prompts, or stale session state are all
//!   treated as no-ops.
//! - Hook state is scoped by Codex `session_id`, not globally across documents.
//!
//! ## Evals
//! - `user_prompt_submit_tracks_agent_doc_file`
//! - `stop_auto_closes_open_cycle_from_last_assistant_message`
//! - `stop_passes_through_committed_cycle`
//! - `stop_blocks_open_cycle_without_recoverable_response`
//! - `stop_fails_closed_after_one_auto_continue`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct UserPromptSubmitInput {
    session_id: String,
    turn_id: String,
    cwd: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct StopInput {
    session_id: String,
    #[allow(dead_code)]
    turn_id: String,
    cwd: String,
    #[serde(default)]
    last_assistant_message: String,
    #[serde(default)]
    stop_hook_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SessionState {
    session_id: String,
    doc_path: String,
    last_turn_id: String,
    updated_at: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(untagged)]
enum StopResponse {
    Continue {
        #[serde(rename = "continue")]
        continue_: bool,
    },
    Block {
        decision: &'static str,
        reason: String,
    },
    Stop {
        #[serde(rename = "continue")]
        continue_: bool,
        #[serde(rename = "stopReason")]
        stop_reason: String,
    },
}

enum StopCloseAttempt {
    Closed,
    StillOpen { note: String },
    NotPossible,
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

pub fn handle_stop() -> Result<()> {
    let response = match read_stdin_payload()
        .and_then(|payload| serde_json::from_str::<StopInput>(&payload).context("parse stop JSON"))
        .and_then(|input| apply_stop(&input))
    {
        Ok(response) => response,
        Err(err) => StopResponse::Stop {
            continue_: false,
            stop_reason: format!("agent-doc Stop hook failed closed: {err}"),
        },
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

fn apply_user_prompt_submit(input: &UserPromptSubmitInput) -> Result<()> {
    let cwd = PathBuf::from(&input.cwd);
    let doc_path = resolve_agent_doc_path(&input.prompt, &cwd).or_else(|| {
        load_state_any(&project_roots_for(&cwd), &input.session_id)
            .ok()
            .flatten()
            .map(|(_, s)| PathBuf::from(s.doc_path))
    });
    let Some(doc_path) = doc_path else {
        return Ok(());
    };
    let roots = tracking_roots(&cwd, Some(&doc_path));
    if roots.is_empty() {
        return Ok(());
    }

    let state = SessionState {
        session_id: input.session_id.clone(),
        doc_path: doc_path.display().to_string(),
        last_turn_id: input.turn_id.clone(),
        updated_at: now_secs(),
    };
    for root in roots {
        save_state(&root, &state)?;
    }
    Ok(())
}

fn apply_stop(input: &StopInput) -> Result<StopResponse> {
    let cwd = PathBuf::from(&input.cwd);
    let roots = project_roots_for(&cwd);
    if roots.is_empty() {
        return Ok(StopResponse::Continue { continue_: true });
    }
    let Some((loaded_root, state)) = load_state_any(&roots, &input.session_id)? else {
        return Ok(StopResponse::Continue { continue_: true });
    };

    let file = PathBuf::from(&state.doc_path);
    let cleanup_roots = tracking_roots(&cwd, Some(&file));
    if !file.exists() {
        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
        return Ok(StopResponse::Continue { continue_: true });
    }

    match crate::session_check::inspect(&file)? {
        crate::session_check::SessionCheckStatus::Ok(_) => {
            clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
            Ok(StopResponse::Continue { continue_: true })
        }
        crate::session_check::SessionCheckStatus::Interrupted(reason) => {
            if !input.stop_hook_active {
                match attempt_stop_closeout(&file, input)? {
                    StopCloseAttempt::Closed => {
                        clear_state_across_roots(&cleanup_roots, &loaded_root, &input.session_id)?;
                        return Ok(StopResponse::Continue { continue_: true });
                    }
                    StopCloseAttempt::StillOpen { note } => {
                        return Ok(StopResponse::Block {
                            decision: "block",
                            reason: format!(
                                "agent-doc Stop hook intercepted an unfinished document cycle for {}. {}{} Do not send the final answer yet. If the response is missing from the document, run `agent-doc repair {}` first. Then finish the commit boundary for this turn and end with `agent-doc session-check {}`.",
                                file.display(),
                                reason,
                                note,
                                file.display(),
                                file.display()
                            ),
                        });
                    }
                    StopCloseAttempt::NotPossible => {}
                }
            }

            let capture_note = if input.stop_hook_active {
                String::new()
            } else {
                capture_assistant_text(&file, input)
            };
            let display = file.display();
            if input.stop_hook_active {
                return Ok(StopResponse::Stop {
                    continue_: false,
                    stop_reason: format!(
                        "agent-doc Stop hook already continued once for {display}, but the cycle is still open. {reason}{capture_note}"
                    ),
                });
            }
            Ok(StopResponse::Block {
                decision: "block",
                reason: format!(
                    "agent-doc Stop hook intercepted an unfinished document cycle for {display}. {reason}{capture_note} Do not send the final answer yet. If the response is missing from the document, run `agent-doc repair {display}` first. Then finish the commit boundary for this turn and end with `agent-doc session-check {display}`."
                ),
            })
        }
    }
}

fn attempt_stop_closeout(file: &Path, input: &StopInput) -> Result<StopCloseAttempt> {
    let has_response = !input.last_assistant_message.trim().is_empty();
    let has_bypassed_patchback =
        crate::session_check::detect_bypassed_response_write(file)?.is_some();
    if !has_response && !has_bypassed_patchback {
        return Ok(StopCloseAttempt::NotPossible);
    }

    let mut note = String::new();
    if has_response {
        crate::recover::save_pending(file, &input.last_assistant_message)?;
        crate::ops_log::log_op(file, "codex_stop_capture_saved");
        note.push_str(
            " The latest assistant text was captured into the pending/capture ledger before auto-close.",
        );
    }

    if crate::recover::run(file)? {
        note.push_str(" The hook replayed the response through the normal write path.");
    }

    if !crate::git::is_in_git_repo(file) {
        note.push_str(
            " The document is not in a git repository, so the hook could not finish the required commit boundary automatically.",
        );
        return Ok(StopCloseAttempt::StillOpen { note });
    }

    if crate::git::commit(file)? {
        note.push_str(" The hook finished the commit boundary automatically.");
    }

    match crate::session_check::inspect(file)? {
        crate::session_check::SessionCheckStatus::Ok(_) => {
            crate::ops_log::log_op(file, "codex_stop_auto_close_success");
            Ok(StopCloseAttempt::Closed)
        }
        crate::session_check::SessionCheckStatus::Interrupted(reason) => {
            crate::ops_log::log_op(file, &format!("codex_stop_auto_close_open reason={reason}"));
            note.push_str(&format!(
                " The cycle remained open after the auto-close attempt: {reason}"
            ));
            Ok(StopCloseAttempt::StillOpen { note })
        }
    }
}

fn capture_assistant_text(file: &Path, input: &StopInput) -> String {
    if input.last_assistant_message.trim().is_empty() {
        return " The hook did not receive a non-empty `last_assistant_message`, so there was nothing to capture before blocking the turn.".to_string();
    }
    match crate::recover::save_pending(file, &input.last_assistant_message) {
        Ok(()) => {
            crate::ops_log::log_op(file, "codex_stop_capture_saved");
            " The latest assistant text was captured into the pending/capture ledger before the turn stopped.".to_string()
        }
        Err(err) => format!(
            " The hook could not capture the final assistant text before blocking the turn: {err}."
        ),
    }
}

fn read_stdin_payload() -> Result<String> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin()
        .read_to_string(&mut payload)
        .context("read hook payload from stdin")?;
    Ok(payload)
}

fn resolve_agent_doc_path(prompt: &str, cwd: &Path) -> Option<PathBuf> {
    let line = prompt.lines().find(|line| !line.trim().is_empty())?.trim();
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let file = match tokens.as_slice() {
        ["agent-doc", file] | ["/agent-doc", file] => Some(*file),
        ["agent-doc", "claim", file] | ["/agent-doc", "claim", file] => Some(*file),
        ["agent-doc", "compact", file] | ["/agent-doc", "compact", file] => Some(*file),
        ["agent-doc", "compact", "exchange", file]
        | ["/agent-doc", "compact", "exchange", file] => Some(*file),
        _ => None,
    }?;
    let path = PathBuf::from(file);
    let joined = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    Some(joined.canonicalize().unwrap_or(joined))
}

#[cfg(test)]
fn project_root_for(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    crate::snapshot::find_project_root(&canonical)
}

fn project_roots_for(path: &Path) -> Vec<PathBuf> {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut roots = Vec::new();
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };

    while let Some(path) = current {
        if path.join(".agent-doc").is_dir() {
            push_unique_root(&mut roots, path.to_path_buf());
        }
        current = path.parent();
    }

    roots
}

fn tracking_roots(cwd: &Path, doc_path: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = project_roots_for(cwd);
    if let Some(doc_path) = doc_path {
        for root in project_roots_for(doc_path) {
            push_unique_root(&mut roots, root);
        }
    }
    roots
}

fn push_unique_root(roots: &mut Vec<PathBuf>, root: PathBuf) {
    if !roots.iter().any(|existing| existing == &root) {
        roots.push(root);
    }
}

fn load_state_any(roots: &[PathBuf], session_id: &str) -> Result<Option<(PathBuf, SessionState)>> {
    for root in roots {
        if let Some(state) = load_state(root, session_id)? {
            return Ok(Some((root.clone(), state)));
        }
    }
    Ok(None)
}

fn clear_state_across_roots(roots: &[PathBuf], loaded_root: &Path, session_id: &str) -> Result<()> {
    let mut all_roots = roots.to_vec();
    push_unique_root(&mut all_roots, loaded_root.to_path_buf());
    for root in all_roots {
        clear_state(&root, session_id)?;
    }
    Ok(())
}

fn load_state(root: &Path, session_id: &str) -> Result<Option<SessionState>> {
    let path = state_path(root, session_id);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let state =
        serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(state))
}

fn save_state(root: &Path, state: &SessionState) -> Result<()> {
    let path = state_path(root, &state.session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn clear_state(root: &Path, session_id: &str) -> Result<()> {
    let path = state_path(root, session_id);
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

fn state_path(root: &Path, session_id: &str) -> PathBuf {
    let hash = crate::ops_log::content_hash(session_id);
    root.join(".agent-doc/codex-hooks/sessions")
        .join(format!("{hash}.json"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as ProcessCommand;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
    }

    fn init_git_repo(root: &Path, tracked: &Path) {
        let relative = tracked.strip_prefix(root).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", relative.to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
    }

    fn write_doc(dir: &tempfile::TempDir) -> PathBuf {
        let doc = dir.path().join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
        doc
    }

    fn write_nested_doc(dir: &tempfile::TempDir) -> PathBuf {
        let nested = dir.path().join("nested");
        fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("task.md");
        let content = "---\nsession: sid\n---\n\n## User\n\nHello\n";
        fs::write(&doc, content).unwrap();
        crate::snapshot::save(&doc, content).unwrap();
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
    }

    #[test]
    fn stop_auto_closes_open_cycle_from_last_assistant_message() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Final assistant response.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });

        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
        assert!(
            !pending.exists(),
            "pending capture should be cleared after recovery"
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Final assistant response."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }
        let log = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["log", "--oneline", "-1"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&log.stdout).contains("agent-doc(task):"),
            "expected auto-close commit, got: {}",
            String::from_utf8_lossy(&log.stdout)
        );
        let root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_auto_closes_open_cycle_across_nested_roots_and_turn_drift() {
        let dir = setup_project();
        let doc = write_nested_doc(&dir);
        init_git_repo(dir.path(), &doc);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();

        apply_user_prompt_submit(&UserPromptSubmitInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            prompt: format!(
                "agent-doc nested/{}",
                doc.file_name().unwrap().to_string_lossy()
            ),
        })
        .unwrap();

        let nested_root = project_root_for(doc.parent().unwrap()).unwrap();
        assert!(
            load_state(&nested_root, "codex-session").unwrap().is_some(),
            "expected state to be mirrored into nested project root"
        );

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-2".to_string(),
            cwd: doc.parent().unwrap().display().to_string(),
            last_assistant_message: "Recovered from nested root drift.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let content = fs::read_to_string(&doc).unwrap();
        assert!(content.contains("Recovered from nested root drift."));
        match crate::session_check::inspect(&doc).unwrap() {
            crate::session_check::SessionCheckStatus::Ok(message) => {
                assert!(message.contains("committed"));
            }
            other => panic!("expected committed session-check status, got {other:?}"),
        }

        let outer_root = project_root_for(dir.path()).unwrap();
        assert!(load_state(&outer_root, "codex-session").unwrap().is_none());
        assert!(load_state(&nested_root, "codex-session").unwrap().is_none());
    }

    #[test]
    fn stop_blocks_open_cycle_without_recoverable_response() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: String::new(),
            stop_hook_active: false,
        })
        .unwrap();

        match response {
            StopResponse::Block { reason, .. } => {
                assert!(reason.contains("unfinished document cycle"));
                assert!(reason.contains("agent-doc repair"));
            }
            other => panic!("expected block response, got {other:?}"),
        }
    }

    #[test]
    fn stop_passes_through_committed_cycle() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit", Some(&original), Some(&original))
            .unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Done.".to_string(),
            stop_hook_active: false,
        })
        .unwrap();

        assert_eq!(response, StopResponse::Continue { continue_: true });
        let pending = crate::snapshot::pending_path_for(&doc).unwrap();
        assert!(!pending.exists());
    }

    #[test]
    fn stop_fails_closed_after_one_auto_continue() {
        let dir = setup_project();
        let doc = write_doc(&dir);
        let original = fs::read_to_string(&doc).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(&original), Some(&original)).unwrap();
        track_doc(&dir, &doc, "turn-1");

        let response = apply_stop(&StopInput {
            session_id: "codex-session".to_string(),
            turn_id: "turn-1".to_string(),
            cwd: dir.path().display().to_string(),
            last_assistant_message: "Still open.".to_string(),
            stop_hook_active: true,
        })
        .unwrap();

        match response {
            StopResponse::Stop {
                continue_: false,
                stop_reason,
            } => {
                assert!(stop_reason.contains("already continued once"));
                assert!(stop_reason.contains("cycle is still open"));
            }
            other => panic!("expected stop response, got {other:?}"),
        }
    }
}

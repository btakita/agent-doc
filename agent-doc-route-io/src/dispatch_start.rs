use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_doc_controller::dispatch::{
    CodexPaneDispatchStartProofFacts, CodexRoutedDispatchStartProofFacts,
    OpenCodePaneDispatchStartProofFacts, RoutedDispatchStartProof,
    classify_codex_routed_dispatch_start_proof, codex_pane_busy_transition_after_acceptance,
    opencode_pane_state_changed_from_idle,
};
use agent_doc_harness::HarnessConfig;
use tmux_router::Tmux;

const CODEX_USER_PROMPT_COMMAND: &str = "agent-doc hook codex-user-prompt-submit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedDispatchStartTracker {
    CodexHook {
        trigger: String,
        previous_session_id: Option<String>,
        previous_turn_id: Option<String>,
        previous_updated_at: Option<u64>,
        pane: Option<String>,
        pre_dispatch_content: Option<String>,
    },
    OpenCodePane {
        pane: String,
        trigger: String,
        pre_dispatch_content: String,
    },
}

pub fn codex_dispatch_start_tracking_enabled(file: &Path) -> bool {
    let user_hooks = codex_user_hooks_path();
    codex_dispatch_start_tracking_enabled_with_user_hooks(file, user_hooks.as_deref())
}

pub(crate) fn codex_dispatch_start_tracking_enabled_with_user_hooks(
    file: &Path,
    user_hooks: Option<&Path>,
) -> bool {
    if user_hooks.is_some_and(codex_hooks_include_agent_doc_tracking) {
        return true;
    }
    codex_tracking_roots(file)
        .into_iter()
        .any(|root| codex_hooks_visible_from_file(file, &root))
}

fn codex_user_hooks_path() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .map(|codex_dir| codex_dir.join("hooks.json"))
}

fn codex_hooks_include_agent_doc_tracking(path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    root.get("hooks")
        .and_then(|hooks| hooks.get("UserPromptSubmit"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|hooks| {
                        hooks.iter().any(|hook| {
                            hook.get("type").and_then(serde_json::Value::as_str) == Some("command")
                                && hook.get("command").and_then(serde_json::Value::as_str)
                                    == Some(CODEX_USER_PROMPT_COMMAND)
                        })
                    })
            })
        })
}

fn codex_hooks_visible_from_file(file: &Path, hook_root: &Path) -> bool {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };

    while let Some(path) = current {
        let codex_path = path.join(".codex");
        if codex_path.exists() {
            return path == hook_root
                && codex_hooks_include_agent_doc_tracking(&codex_path.join("hooks.json"));
        }
        if path == hook_root {
            return codex_hooks_include_agent_doc_tracking(&hook_root.join(".codex/hooks.json"));
        }
        current = path.parent();
    }

    false
}

fn codex_tracking_roots(file: &Path) -> Vec<PathBuf> {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut roots = Vec::new();
    let mut current = if canonical.is_file() {
        canonical.parent()
    } else {
        Some(canonical.as_path())
    };

    while let Some(path) = current {
        if path.join(".agent-doc").is_dir() {
            roots.push(path.to_path_buf());
        }
        current = path.parent();
    }

    roots
}

pub fn build_routed_dispatch_start_tracker(
    file: &Path,
    file_path: &str,
    harness: &HarnessConfig,
    tmux: Option<&Tmux>,
    pane: Option<&str>,
) -> Result<Option<RoutedDispatchStartTracker>> {
    match harness.binary.as_str() {
        "codex" if codex_dispatch_start_tracking_enabled(file) => {
            let latest = agent_doc_codex_hook_io::load_latest_prompt_state_for_file(file)?;
            let (pane, pre_dispatch_content) = match (tmux, pane) {
                (Some(tmux), Some(pane)) => match agent_doc_tmux_io::capture_pane(tmux, pane) {
                    Ok(content) => (Some(pane.to_string()), Some(content)),
                    Err(err) => {
                        eprintln!(
                            "[route] warning: failed to capture Codex pane {} before routed dispatch: {}",
                            pane, err
                        );
                        (None, None)
                    }
                },
                _ => (None, None),
            };
            Ok(Some(RoutedDispatchStartTracker::CodexHook {
                trigger: harness.trigger_command(file_path),
                previous_session_id: latest.as_ref().map(|state| state.session_id.clone()),
                previous_turn_id: latest.as_ref().map(|state| state.last_turn_id.clone()),
                previous_updated_at: latest.as_ref().map(|state| state.updated_at),
                pane,
                pre_dispatch_content,
            }))
        }
        "opencode" => {
            let (Some(tmux), Some(pane)) = (tmux, pane) else {
                return Ok(None);
            };
            let pre_dispatch_content =
                agent_doc_tmux_io::capture_pane(tmux, pane).with_context(|| {
                    format!(
                        "failed to capture OpenCode pane {} before routed dispatch",
                        pane
                    )
                })?;
            Ok(Some(RoutedDispatchStartTracker::OpenCodePane {
                pane: pane.to_string(),
                trigger: harness.trigger_command(file_path),
                pre_dispatch_content,
            }))
        }
        _ => Ok(None),
    }
}

fn codex_routed_dispatch_start_proof_facts<'a>(
    tracker: &'a RoutedDispatchStartTracker,
    state: &'a agent_doc_codex_hook_io::ActiveSessionState,
) -> Option<CodexRoutedDispatchStartProofFacts<'a>> {
    let RoutedDispatchStartTracker::CodexHook {
        trigger,
        previous_session_id,
        previous_turn_id,
        previous_updated_at,
        ..
    } = tracker
    else {
        return None;
    };
    Some(CodexRoutedDispatchStartProofFacts {
        trigger,
        previous_session_id: previous_session_id.as_deref(),
        previous_turn_id: previous_turn_id.as_deref(),
        previous_updated_at: *previous_updated_at,
        current_session_id: state.session_id.as_str(),
        current_turn_id: state.last_turn_id.as_str(),
        current_updated_at: state.updated_at,
        current_prompt: state.last_prompt.as_str(),
    })
}

fn codex_pane_dispatch_start_proof_facts<'a>(
    harness: &HarnessConfig,
    pre_dispatch_content: &'a str,
    current_content: &'a str,
) -> CodexPaneDispatchStartProofFacts<'a> {
    CodexPaneDispatchStartProofFacts {
        pre_dispatch_content,
        current_content,
        pre_dispatch_has_busy_cue: harness.codex_active_turn_is_busy(pre_dispatch_content),
        current_has_busy_cue: harness.codex_active_turn_is_busy(current_content),
    }
}

fn opencode_pane_dispatch_start_proof_facts<'a>(
    harness: &HarnessConfig,
    trigger: &'a str,
    pre_dispatch_content: &'a str,
    current_content: &'a str,
) -> OpenCodePaneDispatchStartProofFacts<'a> {
    OpenCodePaneDispatchStartProofFacts {
        trigger,
        pre_dispatch_content,
        current_content,
        current_has_ready_prompt_candidate: agent_doc_harness::ready_prompt_candidate(
            current_content,
            harness,
        )
        .is_some(),
        current_is_idle_chrome_only_output: harness.is_idle_chrome_only_output(current_content),
        current_has_busy_cue: harness.has_busy_cue(current_content),
        current_has_non_idle_output_line: current_content
            .lines()
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
            .any(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty()
                    && !harness.is_ignorable_output_line(trimmed)
                    && !harness.is_dispatch_ready_prompt_line(trimmed)
            }),
    }
}

pub fn wait_for_routed_dispatch_start(
    tmux: &Tmux,
    file: &Path,
    tracker: &RoutedDispatchStartTracker,
    harness: &HarnessConfig,
    timeout: Duration,
) -> Result<Option<RoutedDispatchStartProof>> {
    let start = std::time::Instant::now();
    let poll = if matches!(tracker, RoutedDispatchStartTracker::OpenCodePane { .. }) {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(200)
    };

    while start.elapsed() < timeout {
        match tracker {
            RoutedDispatchStartTracker::CodexHook {
                pane,
                pre_dispatch_content,
                ..
            } => {
                if let Some(state) =
                    agent_doc_codex_hook_io::load_latest_prompt_state_for_file(file)?
                    && let Some(facts) = codex_routed_dispatch_start_proof_facts(tracker, &state)
                    && let Some(proof) = classify_codex_routed_dispatch_start_proof(facts)
                {
                    return Ok(Some(proof));
                }
                if let (Some(pane), Some(pre_dispatch_content)) =
                    (pane.as_deref(), pre_dispatch_content.as_deref())
                {
                    let content = agent_doc_tmux_io::capture_pane(tmux, pane).with_context(|| {
                        format!(
                            "failed to capture Codex pane {} while awaiting routed dispatch proof",
                            pane
                        )
                    })?;
                    let facts = codex_pane_dispatch_start_proof_facts(
                        harness,
                        pre_dispatch_content,
                        &content,
                    );
                    if codex_pane_busy_transition_after_acceptance(facts) {
                        return Ok(Some(RoutedDispatchStartProof::PaneStateChanged));
                    }
                }
            }
            RoutedDispatchStartTracker::OpenCodePane {
                pane,
                trigger,
                pre_dispatch_content,
            } => {
                let content = agent_doc_tmux_io::capture_pane(tmux, pane).with_context(|| {
                    format!(
                        "failed to capture OpenCode pane {} while awaiting routed dispatch proof",
                        pane
                    )
                })?;
                let facts = opencode_pane_dispatch_start_proof_facts(
                    harness,
                    trigger,
                    pre_dispatch_content,
                    &content,
                );
                if opencode_pane_state_changed_from_idle(facts) {
                    return Ok(Some(RoutedDispatchStartProof::PaneStateChanged));
                }
            }
        }
        std::thread::sleep(poll);
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_pane_facts_recognize_working_snapshot_after_idle_baseline() {
        let harness = HarnessConfig::codex();
        let before = "\
›

  gpt-5.6 · ~/work/sample-app · 40% left
";
        let active = "\
› agent-doc /work/sample-app/tasks/sampleorders.md

• I’m opening the Agent Doc session and checking the repository context first.

• Ran tsift status
  └ Index status: fresh

• Working (4s • esc to interrupt)

› Write tests for @filename
";
        let facts = codex_pane_dispatch_start_proof_facts(&harness, before, active);

        assert!(!facts.pre_dispatch_has_busy_cue);
        assert!(facts.current_has_busy_cue);
        assert!(codex_pane_busy_transition_after_acceptance(facts));
    }

    #[test]
    fn codex_pane_facts_ignore_stale_working_scrollback_outside_active_turn_window() {
        let harness = HarnessConfig::codex();
        let before = "\
• Working (31s • esc to interrupt)
• Previous turn completed.
• Ran agent-doc session.md
  └ agent-doc completed
• Retained response was delivered.

›

gpt-5.6 · ~/work/sample-app · 40% left
";
        let active = "\
• Working (31s • esc to interrupt)
• Previous turn completed.
• Ran agent-doc session.md
  └ agent-doc completed
• Retained response was delivered.

› agent-doc session.md

• I’m opening the session document.
• Working (2s • esc to interrupt)
";
        let facts = codex_pane_dispatch_start_proof_facts(&harness, before, active);

        assert!(
            !facts.pre_dispatch_has_busy_cue,
            "proof must use the same bottom active-turn window that admitted the idle pane"
        );
        assert!(facts.current_has_busy_cue);
        assert!(codex_pane_busy_transition_after_acceptance(facts));
    }

    #[test]
    fn codex_pane_facts_recognize_padded_working_snapshot_after_idle_baseline() {
        let harness = HarnessConfig::codex();
        let mut before = "\
• Working (31s • esc to interrupt)
• Previous turn completed.
• Ran agent-doc session.md
  └ agent-doc completed
• Retained response was delivered.

›

gpt-5.6 · ~/work/sample-app · 40% left"
            .to_string();
        before.push_str(&"\n".repeat(12));
        let mut active = "\
› agent-doc /work/sample-app/tasks/session.md

• I’m opening the Agent Doc session and checking the repository context first.

• Ran tsift status
  └ Index status: fresh

• Working (3s • esc to interrupt)


› Write tests for @filename

gpt-5.6 · ~/work/sample-app · 40% left"
            .to_string();
        active.push_str(&"\n".repeat(12));
        let facts = codex_pane_dispatch_start_proof_facts(&harness, &before, &active);

        assert!(!facts.pre_dispatch_has_busy_cue);
        assert!(facts.current_has_busy_cue);
        assert!(codex_pane_busy_transition_after_acceptance(facts));
    }

    #[test]
    fn codex_dispatch_start_tracking_enabled_accepts_workspace_hook_for_nested_agent_doc_root() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        let nested = workspace.join("src/session-share");
        let doc = nested.join("tasks/claudescore-3.md");

        std::fs::create_dir_all(workspace.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(workspace.join(".codex")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            workspace.join(".codex/hooks.json"),
            serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [{
                        "hooks": [{
                            "type": "command",
                            "command": CODEX_USER_PROMPT_COMMAND,
                        }]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            codex_dispatch_start_tracking_enabled_with_user_hooks(&doc, None),
            "workspace-level Codex hooks should enable routed dispatch tracking for nested agent-doc roots"
        );
    }

    #[test]
    fn codex_dispatch_start_tracking_enabled_stays_false_without_any_hook_install() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("src/session-share");
        let doc = nested.join("tasks/claudescore-3.md");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            !codex_dispatch_start_tracking_enabled_with_user_hooks(&doc, None),
            "route should not wait for hook-backed submission proof when no tracked root has Codex hooks installed"
        );
    }

    #[test]
    fn codex_dispatch_start_tracking_enabled_stays_false_when_nested_codex_path_shadows_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        let nested = workspace.join("src/session-share");
        let doc = nested.join("tasks/claudescore-3.md");

        std::fs::create_dir_all(workspace.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(workspace.join(".codex")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            workspace.join(".codex/hooks.json"),
            serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [{
                        "hooks": [{
                            "type": "command",
                            "command": CODEX_USER_PROMPT_COMMAND,
                        }]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(nested.join(".codex"), "").unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();

        assert!(
            !codex_dispatch_start_tracking_enabled_with_user_hooks(&doc, None),
            "route should not require hook-backed submission proof when a nearer `.codex` path shadows the workspace install"
        );
    }

    #[test]
    fn codex_dispatch_start_tracking_uses_user_hook_across_nested_git_roots() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("src/haiven-dev");
        let doc = nested.join("tasks/backend.md");
        let user_hooks = dir.path().join("codex-home/hooks.json");

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::create_dir_all(user_hooks.parent().unwrap()).unwrap();
        std::fs::write(nested.join(".codex"), "").unwrap();
        std::fs::write(&doc, "# Session\n").unwrap();
        std::fs::write(
            &user_hooks,
            serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [{
                        "hooks": [{
                            "type": "command",
                            "command": CODEX_USER_PROMPT_COMMAND,
                        }]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();

        assert!(
            codex_dispatch_start_tracking_enabled_with_user_hooks(&doc, Some(&user_hooks)),
            "the user hook remains visible when nested git roots or `.codex` paths hide project hooks"
        );
    }
}

//! # Module: prompt
//!
//! ## Spec
//! - Detects active Claude Code and OpenCode permission/question prompts in a tmux pane and surfaces them as JSON.
//! - `run(file)` — resolves the session's tmux pane from the document frontmatter, captures pane content, parses for prompt patterns, and prints a `PromptInfo` JSON object to stdout.
//! - `run_all()` — iterates every entry in `sessions.json`, skips dead panes, and prints a JSON array of `PromptAllEntry` objects (one per live session with its prompt state).
//! - `answer(file, option_index)` — navigates the prompt TUI to the target option using the prompt's axis (Claude Code: Up/Down, OpenCode: Tab/BackTab; 30 ms between presses) then sends Enter. OpenCode `Allow always` sends a second Enter to accept the follow-up confirmation prompt. Validates that a prompt is active and the index is in range before sending keys.
//! - Pure prompt parsing, ANSI stripping, and navigation-key policy live in `agent-doc-turn-executor-tmux::prompt`; this module only supplies session/frontmatter/tmux adapters.
//! - `selected` is 0-based (reflecting TUI cursor position); `options[*].index` is 1-based (matching the TUI display).
//! - When no pane is registered or the pane is dead, `run` emits `{"active":false}` and returns `Ok(())`.
//! - Optional fields (`question`, `options`, `selected`) are omitted from JSON serialization when `None`.
//!
//! ## Agentic Contracts
//! - `run(file)` / `run_with_tmux(file, tmux)` — returns `Err` only on file I/O or tmux command failure; missing/dead pane produces `active: false` output, not an error.
//! - `answer(file, option_index)` — returns `Err` when: file missing, no pane registered, pane dead, no active prompt, or index out of range (1-based).
//! - `PromptAllEntry` serializes flat (prompt fields at top level via `#[serde(flatten)]`).
//!
//! ## Evals
//! - prompt_all_entry_serializes_flat: active entry → `session_id`, `file`, `active`, `question` all at JSON top level
//! - prompt_all_entry_inactive_omits_optional: inactive entry → no `question`/`options` keys in JSON

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_frontmatter::frontmatter;
use agent_doc_turn_executor_tmux::prompt::{
    PromptInfo, PromptNavigationAxis, inactive_prompt, navigation_axis_for_prompt,
    navigation_keys_for_prompt, opencode_option_requires_confirmation, parse_prompt, strip_ansi,
};
use serde::Serialize;
use tmux_router::{Registry as SessionRegistry, Tmux};

use crate::sessions;

pub fn run(file: &Path) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server())
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (_updated, session_id) = frontmatter::ensure_session(&content)?;

    let pane = sessions::lookup(&session_id)?;
    let pane_id = match pane {
        Some(p) => p,
        None => {
            let info = PromptInfo {
                active: false,
                question: None,
                options: None,
                selected: None,
            };
            println!("{}", serde_json::to_string(&info)?);
            return Ok(());
        }
    };

    if !tmux.pane_alive(&pane_id) {
        let info = PromptInfo {
            active: false,
            question: None,
            options: None,
            selected: None,
        };
        println!("{}", serde_json::to_string(&info)?);
        return Ok(());
    }

    let pane_content = sessions::capture_pane_with_ansi(tmux, &pane_id)?;
    let info = parse_prompt(&pane_content);
    println!("{}", serde_json::to_string(&info)?);
    Ok(())
}

/// Entry in the `--all` output: one per live session.
#[derive(Debug, Serialize)]
pub struct PromptAllEntry {
    pub session_id: String,
    pub file: String,
    pub cwd: String,
    #[serde(flatten)]
    pub prompt: PromptInfo,
}

/// Poll all live sessions for active prompts.
pub fn run_all() -> Result<()> {
    run_all_with_tmux(&Tmux::default_server())
}

pub fn run_all_with_tmux(tmux: &Tmux) -> Result<()> {
    let registry: SessionRegistry = sessions::load()?;
    let mut entries: Vec<PromptAllEntry> = Vec::new();
    let verbose = std::env::var("AGENT_DOC_PROMPT_DEBUG").is_ok();

    for entry in registry.values() {
        if !tmux.pane_alive(&entry.pane) {
            if verbose {
                eprintln!(
                    "[prompt] pane {} dead for session {} ({})",
                    entry.pane, entry.session_id, entry.file
                );
            }
            continue;
        }

        let prompt = match sessions::capture_pane_with_ansi(tmux, &entry.pane) {
            Ok(content) => {
                if verbose {
                    // Log the last 5 non-empty lines for debugging prompt detection
                    let last_lines: Vec<&str> = content
                        .lines()
                        .rev()
                        .filter(|l| !l.trim().is_empty())
                        .take(5)
                        .collect();
                    eprintln!("[prompt] pane {} ({}) last lines:", entry.pane, entry.file);
                    for line in last_lines.iter().rev() {
                        eprintln!("[prompt]   {}", strip_ansi(line));
                    }
                }
                parse_prompt(&content)
            }
            Err(e) => {
                if verbose {
                    eprintln!("[prompt] capture failed for pane {}: {}", entry.pane, e);
                }
                inactive_prompt()
            }
        };

        if verbose {
            eprintln!(
                "[prompt] session {} active={} question={:?}",
                entry.session_id, prompt.active, prompt.question
            );
        }

        entries.push(PromptAllEntry {
            session_id: entry.session_id.clone(),
            file: entry.file.clone(),
            cwd: entry.cwd.clone(),
            prompt,
        });
    }

    println!("{}", serde_json::to_string(&entries)?);
    Ok(())
}

pub fn answer(file: &Path, option_index: usize) -> Result<()> {
    answer_with_tmux(file, option_index, &Tmux::default_server())
}

pub fn answer_with_tmux(file: &Path, option_index: usize, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (_updated, session_id) = frontmatter::ensure_session(&content)?;

    let pane = sessions::lookup(&session_id)?;
    let pane_id = pane.context("no pane registered for this session")?;

    if !tmux.pane_alive(&pane_id) {
        anyhow::bail!("pane {} is not alive", pane_id);
    }

    // Verify there's actually a prompt active
    let pane_content = sessions::capture_pane_with_ansi(tmux, &pane_id)?;
    let info = parse_prompt(&pane_content);
    if !info.active {
        anyhow::bail!("no active prompt detected");
    }

    let options = info.options.as_ref().unwrap();
    if option_index == 0 || option_index > options.len() {
        anyhow::bail!("option {} out of range (1-{})", option_index, options.len());
    }

    // Navigate to the selected option and press Enter. Claude Code uses a
    // vertical menu. OpenCode's permission prompt advertises the tab selector
    // in its footer; real arrow keys can leak as literal ^[[C/^[[D text when
    // the surrounding terminal does not honor OpenTUI's keyboard mode.
    let current = info.selected.unwrap_or(0);
    let target = option_index - 1; // convert to 0-based
    let keys = navigation_keys_for_prompt(&pane_content);

    if target < current {
        for _ in 0..(current - target) {
            sessions::send_key(tmux, &pane_id, keys.prev)?;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    } else if target > current {
        for _ in 0..(target - current) {
            sessions::send_key(tmux, &pane_id, keys.next)?;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }

    // Brief pause then press Enter to confirm
    std::thread::sleep(std::time::Duration::from_millis(50));
    sessions::send_key(tmux, &pane_id, "Enter")?;
    if navigation_axis_for_prompt(&pane_content) == PromptNavigationAxis::Horizontal
        && opencode_option_requires_confirmation(&options[target])
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
        sessions::send_key(tmux, &pane_id, "Enter")?;
    }

    eprintln!("Sent option {} to pane {}", option_index, pane_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn_executor_tmux::prompt::PromptOption;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    struct ScopedCurrentDir {
        prev_cwd: PathBuf,
        _env_guard: crate::test_support::ProcessGlobalLockGuard,
    }

    impl ScopedCurrentDir {
        fn set(path: &Path) -> Self {
            let env_guard = crate::test_support::env_lock();
            let prev_cwd = std::env::current_dir()
                .ok()
                .filter(|cwd| cwd.exists())
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            std::env::set_current_dir(path).unwrap();
            Self {
                prev_cwd,
                _env_guard: env_guard,
            }
        }
    }

    impl Drop for ScopedCurrentDir {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.prev_cwd);
        }
    }

    fn wait_for<F>(timeout: Duration, mut predicate: F) -> bool
    where
        F: FnMut() -> bool,
    {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        predicate()
    }

    #[test]
    fn prompt_all_entry_serializes_flat() {
        let entry = PromptAllEntry {
            session_id: "abc-123".to_string(),
            file: "tasks/plan.md".to_string(),
            cwd: "/repo".to_string(),
            prompt: PromptInfo {
                active: true,
                question: Some("Allow?".to_string()),
                options: Some(vec![
                    PromptOption {
                        index: 1,
                        label: "Yes".to_string(),
                    },
                    PromptOption {
                        index: 2,
                        label: "No".to_string(),
                    },
                ]),
                selected: Some(0),
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"session_id\":\"abc-123\""));
        assert!(json.contains("\"file\":\"tasks/plan.md\""));
        assert!(json.contains("\"cwd\":\"/repo\""));
        assert!(json.contains("\"active\":true"));
        assert!(json.contains("\"question\":\"Allow?\""));
    }

    #[test]
    fn prompt_all_entry_inactive_omits_optional_fields() {
        let entry = PromptAllEntry {
            session_id: "def-456".to_string(),
            file: "tasks/resume.md".to_string(),
            cwd: "/repo".to_string(),
            prompt: inactive_prompt(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"active\":false"));
        assert!(!json.contains("\"question\""));
        assert!(!json.contains("\"options\""));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn answer_opencode_prompt_sends_tab_not_arrow_escape() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd = ScopedCurrentDir::set(tmp.path());
        let session_id = "prompt-opencode-tab-session";
        let doc = tmp.path().join("prompt.md");
        std::fs::write(
            &doc,
            format!("---\nagent_doc_session: {session_id}\n---\n# Prompt test\n"),
        )
        .unwrap();

        let key_log = tmp.path().join("keys.bin");
        let script = tmp.path().join("mock-opencode-prompt.sh");
        std::fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env bash
printf '\033[48;2;245;167;66mAllow once\033[0m Allow always Reject ⇆ select enter confirm\n'
dd of='{}' bs=1 count=3 status=none
sleep 1
"#,
                key_log.display()
            ),
        )
        .unwrap();

        let tmux = tmux_router::IsolatedTmux::new("prompt-opencode-tab-answer");
        let pane = tmux
            .new_session("prompt-opencode-tab-answer", tmp.path())
            .unwrap();
        tmux.send_keys(&pane, &format!("bash {}", script.display()))
            .unwrap();

        assert!(
            wait_for(Duration::from_secs(3), || {
                crate::sessions::capture_pane_with_ansi(&tmux, &pane)
                    .map(|content| parse_prompt(&content).active)
                    .unwrap_or(false)
            }),
            "mock OpenCode prompt did not become active"
        );

        let mut registry = tmux_router::Registry::new();
        let key = crate::sessions::canonical_registry_key_in(tmp.path(), "prompt.md");
        registry.insert(
            key,
            tmux_router::RegistryEntry {
                pane: pane.clone(),
                pid: std::process::id(),
                cwd: tmp.path().to_string_lossy().to_string(),
                started: "2026-05-13T00:00:00Z".to_string(),
                session_id: session_id.to_string(),
                file: "prompt.md".to_string(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        crate::sessions::save(&registry).unwrap();

        answer_with_tmux(&doc, 2, &tmux).unwrap();

        assert!(
            wait_for(Duration::from_secs(3), || key_log.exists()),
            "mock prompt did not record received keys"
        );
        let keys = std::fs::read(key_log).unwrap();
        assert_eq!(keys.first().copied(), Some(b'\t'));
        assert_ne!(keys.first().copied(), Some(0x1b), "sent an arrow escape");
    }
}

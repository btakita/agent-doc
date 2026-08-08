//! The effect half of the in-binary preflight hook (`#preflightinbinary`).
//!
//! The decision — "is this prompt an `agent-doc <FILE>` trigger, and which
//! document?" — lives in
//! [`agent_doc_hooks_io::preflight_user_prompt_submit`], a leaf crate with no
//! preflight dependency. This is the part that runs preflight, and it lives in
//! the binary because `agent-doc-commit-io` already depends on the hooks crate
//! and preflight depends on commit: calling preflight from the leaf would be a
//! cycle. Pure decision at the bottom, effect at the top.

use std::path::{Path, PathBuf};

use agent_doc_hooks_io::preflight_user_prompt_submit::{invoked_document, resolve_document};

/// Printed after the injected contract has been produced successfully.
///
/// `SKILL.md` keys "do not run `agent-doc preflight` yourself" off this line, so
/// the agent tests for a fact rather than guessing whether the hook ran. Keeping
/// this as the final seal prevents partial preflight output from being mistaken
/// for an admitted cycle.
pub const CONTRACT_MARKER: &str = "[agent-doc] cycle contract (preflight already ran in the binary; do NOT run `agent-doc preflight` for this turn)";

fn read_stdin_payload() -> anyhow::Result<String> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    Ok(payload)
}

fn run_preflight_for_prompt(prompt: &str, cwd: &Path) -> anyhow::Result<()> {
    let Some(target) = invoked_document(prompt) else {
        return Ok(());
    };
    let Some(file) = resolve_document(cwd, &target) else {
        return Ok(());
    };

    agent_doc_preflight_command_io::run_with_options(
        &file,
        agent_doc_preflight_command_io::PreflightOptions { probe: false },
    )?;

    // The marker seals a successfully produced contract. It must not appear on
    // any error path because the skill treats its absence as failed admission.
    println!("{CONTRACT_MARKER}");
    Ok(())
}

/// Claude Code `UserPromptSubmit` hook entry point.
///
/// Reads the hook payload from stdin. When the prompt is an `agent-doc <FILE>`
/// trigger, runs the preflight pipeline in-process; its stdout is the cycle
/// contract, which Claude Code injects as context for the turn about to start —
/// so the agent receives the contract *with* the prompt instead of having to
/// remember to shell back for it.
///
/// The hook process remains best-effort so ordinary non-agent-doc prompts are
/// never blocked. The agent-doc skill fails closed when this hook does not emit
/// a contract; the model never recreates admission by invoking preflight.
pub fn handle_user_prompt_submit() -> anyhow::Result<()> {
    let payload = match read_stdin_payload() {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!("[agent-doc] preflight hook payload read failed: {err:#}");
            return Ok(());
        }
    };
    let input = match serde_json::from_str::<serde_json::Value>(&payload) {
        Ok(input) => input,
        Err(err) => {
            eprintln!("[agent-doc] preflight hook JSON parse failed: {err}");
            return Ok(());
        }
    };
    let prompt = input
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let cwd = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if let Err(err) = run_preflight_for_prompt(prompt, &cwd) {
        // stderr, not stdout: stdout is injected context, and a diagnostic
        // must not be mistaken for a cycle contract.
        eprintln!("[agent-doc] preflight hook failed: {err:#}");
    }
    Ok(())
}

/// Codex `UserPromptSubmit` entry point.
///
/// Codex installs one hook command, so admission and session tracking are kept
/// in this one binary-owned transaction. Tracking runs first: if it cannot be
/// made durable, preflight does not open a cycle that the Stop hook cannot find.
/// A successful preflight then writes the contract and its final marker to
/// stdout, which Codex injects as additional context for the arriving turn.
pub fn handle_codex_user_prompt_submit() -> anyhow::Result<()> {
    let payload = match read_stdin_payload() {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!("[agent-doc] Codex user-prompt-submit payload read failed: {err:#}");
            return Ok(());
        }
    };
    let input =
        match serde_json::from_str::<agent_doc_codex_hook_io::UserPromptSubmitInput>(&payload) {
            Ok(input) => input,
            Err(err) => {
                eprintln!("[agent-doc] Codex user-prompt-submit JSON parse failed: {err}");
                return Ok(());
            }
        };

    if let Err(err) = agent_doc_codex_hook_io::apply_user_prompt_submit(&input) {
        eprintln!("[agent-doc] Codex session tracking failed: {err:#}");
        return Ok(());
    }
    if let Err(err) = run_preflight_for_prompt(&input.prompt, Path::new(&input.cwd)) {
        eprintln!("[agent-doc] Codex preflight hook failed: {err:#}");
    }
    Ok(())
}

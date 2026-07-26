//! The effect half of the in-binary preflight hook (`#preflightinbinary`).
//!
//! The decision — "is this prompt an `agent-doc <FILE>` trigger, and which
//! document?" — lives in
//! [`agent_doc_hooks_io::preflight_user_prompt_submit`], a leaf crate with no
//! preflight dependency. This is the part that runs preflight, and it lives in
//! the binary because `agent-doc-commit-io` already depends on the hooks crate
//! and preflight depends on commit: calling preflight from the leaf would be a
//! cycle. Pure decision at the bottom, effect at the top.

use std::path::PathBuf;

use agent_doc_hooks_io::preflight_user_prompt_submit::{invoked_document, resolve_document};

/// Claude Code `UserPromptSubmit` hook entry point.
///
/// Reads the hook payload from stdin. When the prompt is an `agent-doc <FILE>`
/// trigger, runs the preflight pipeline in-process; its stdout is the cycle
/// contract, which Claude Code injects as context for the turn about to start —
/// so the agent receives the contract *with* the prompt instead of having to
/// remember to shell back for it.
///
/// Fails open in every branch. A hook that breaks a turn is worse than a hook
/// that does nothing, and the agent still has `agent-doc preflight` as the
/// explicit path.
pub fn handle_user_prompt_submit() -> anyhow::Result<()> {
    use std::io::Read;
    let mut payload = String::new();
    if std::io::stdin().read_to_string(&mut payload).is_err() {
        return Ok(());
    }
    let Ok(input) = serde_json::from_str::<serde_json::Value>(&payload) else {
        return Ok(());
    };
    let prompt = input
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let Some(target) = invoked_document(prompt) else {
        return Ok(());
    };
    let cwd = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let Some(file) = resolve_document(&cwd, &target) else {
        return Ok(());
    };

    if let Err(e) = agent_doc_preflight_command_io::run_with_options(
        &file,
        agent_doc_preflight_command_io::PreflightOptions { probe: false },
    ) {
        // stderr, not stdout: stdout is the injected context, and a diagnostic
        // must not be mistaken for a cycle contract.
        eprintln!(
            "[agent-doc] preflight hook skipped for {}: {e:#}",
            file.display()
        );
    }
    Ok(())
}

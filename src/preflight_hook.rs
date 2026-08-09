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

/// Printed to **stdout** when the prompt *was* an `agent-doc <FILE>` trigger but
/// preflight could not produce a contract (`#hookcontractlost`).
///
/// Silence is the failure mode this exists to remove. The hook's diagnostics used
/// to go only to stderr, but a `UserPromptSubmit` hook injects **stdout** as turn
/// context — so a preflight error reached the operator's log and never the agent,
/// which then saw an ordinary prompt with no contract and no reason. Observed
/// 2026-08-09 across three consecutive turns: preflight opened a cycle, bailed on
/// snapshot/HEAD drift, and the agent was left with nothing to distinguish "the
/// hook never ran" from "preflight refused".
///
/// Emitting this marker keeps those three states separable:
/// - [`CONTRACT_MARKER`] present → admitted, contract above it is authoritative
/// - this marker present → the hook ran, preflight refused, reason + remedy follow
/// - neither present → the hook did not run at all (a harness-wiring defect)
///
/// It is deliberately NOT the contract marker: admission still failed, so the
/// agent must not proceed as though a cycle were opened.
pub const ADMISSION_FAILURE_MARKER: &str =
    "[agent-doc] cycle contract UNAVAILABLE (preflight admission failed; do NOT run `agent-doc preflight` for this turn)";

fn read_stdin_payload() -> anyhow::Result<String> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    Ok(payload)
}

/// Emit a machine-readable admission failure as injected turn context.
///
/// stdout only, on purpose — see [`ADMISSION_FAILURE_MARKER`]. Callers log their
/// own stderr diagnostic for the operator's hook log, because the stderr copy is
/// wanted even for prompts that never reach this function.
fn emit_admission_failure(target: &str, err: &anyhow::Error) {
    println!("{ADMISSION_FAILURE_MARKER}");
    println!("document: {target}");
    println!("reason: {err:#}");
    println!(
        "remedy: preflight refused to admit this turn, so no cycle contract exists. \
         Do NOT shell `agent-doc preflight` to recreate admission. Report this failure \
         and its reason to the operator, and stop."
    );
}

/// Outcome of a hook admission attempt.
///
/// Returned rather than propagated because every branch is already fully
/// reported to the agent (stdout) and the operator (stderr) — there is no
/// residual error for a caller to handle, and a hook must never block an
/// ordinary prompt. This keeps the "never swallow errors" rule honest: nothing
/// is discarded, the reporting simply happens where the remedy is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookAdmission {
    /// The prompt was not an `agent-doc <FILE>` trigger; the hook is a no-op.
    NotATrigger,
    /// Preflight produced a contract, sealed by [`CONTRACT_MARKER`].
    Admitted,
    /// Preflight refused; [`ADMISSION_FAILURE_MARKER`] and its reason were emitted.
    Failed,
}

fn run_preflight_for_prompt(prompt: &str, cwd: &Path) -> HookAdmission {
    let Some(target) = invoked_document(prompt) else {
        return HookAdmission::NotATrigger;
    };
    let Some(file) = resolve_document(cwd, &target) else {
        return HookAdmission::NotATrigger;
    };

    if let Err(err) = agent_doc_preflight_command_io::run_with_options(
        &file,
        agent_doc_preflight_command_io::PreflightOptions { probe: false },
    ) {
        // A trigger that reached preflight must never produce silence: the agent
        // cannot tell an absent hook from a refusing one, and the refusal is the
        // only place the remedy is known.
        eprintln!("[agent-doc] preflight hook failed: {err:#}");
        emit_admission_failure(&file.display().to_string(), &err);
        return HookAdmission::Failed;
    }

    // The marker seals a successfully produced contract. It must not appear on
    // any error path because the skill treats its absence as failed admission.
    println!("{CONTRACT_MARKER}");
    HookAdmission::Admitted
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

    // Every outcome is already reported to the agent and the operator inside
    // `run_preflight_for_prompt`, so a refusal never blocks an ordinary prompt.
    run_preflight_for_prompt(prompt, &cwd);
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
        // Tracking failed, so preflight must not open a cycle the Stop hook
        // cannot find. When this prompt *was* an `agent-doc <FILE>` trigger the
        // agent would otherwise see silence, so name the refusal
        // (`#hookcontractlost`). Ordinary prompts stay untouched.
        if invoked_document(&input.prompt).is_some() {
            emit_admission_failure(&input.cwd, &err.context("Codex session tracking failed"));
        }
        return Ok(());
    }
    run_preflight_for_prompt(&input.prompt, Path::new(&input.cwd));
    Ok(())
}

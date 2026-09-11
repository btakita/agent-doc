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

use agent_doc_hooks_io::preflight_user_prompt_submit::{
    AgentDocInvocation, invoked_agent_doc, invoked_document, resolve_document,
};

/// Printed after the injected contract has been produced successfully.
///
/// `SKILL.md` keys "do not run `agent-doc preflight` yourself" off this line, so
/// the agent tests for a fact rather than guessing whether the hook ran. Keeping
/// this as the final seal prevents partial preflight output from being mistaken
/// for an admitted cycle.
pub const CONTRACT_MARKER: &str = "[agent-doc] cycle contract (preflight already ran in the binary; do NOT run `agent-doc preflight` for this turn)";
const CODEX_IN_PANE_ADMISSION_DIRECTIVE: &str = "[agent-doc] Codex in-pane admission: continue this response cycle in the current turn. Do NOT execute `agent-doc <FILE>` as a shell command; that would recursively re-enter the owning pane. When `owned_pane_self_invocation` is non-null, execute its unresolved work now; do not reply that the document is merely already active or ask the operator to resend the task.";

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
pub const ADMISSION_FAILURE_MARKER: &str = "[agent-doc] cycle contract UNAVAILABLE (preflight admission failed; do NOT run `agent-doc preflight` for this turn)";

/// Wall-clock budget for one in-binary preflight admission.
///
/// Preflight is not fast on a large document — measured 2026-08-09 at 23-34s on
/// an 87KB session document at ~10% CPU, i.e. almost entirely blocked on
/// controller round trips (`#preflightprojpass`). Claude Code's default hook
/// timeout is 30s, and on expiry it **discards the hook's output**, which
/// reproduces `#hookcontractlost` exactly: no contract, no reason, and nothing
/// to distinguish a slow hook from an unwired one.
///
/// Two things keep that from happening. The installed hook entry carries an
/// explicit [`crate::skill::PREFLIGHT_HOOK_TIMEOUT_SECS`] so the harness stops
/// killing a merely-slow preflight, and this budget — deliberately *under* that
/// timeout — lets the binary name its own overrun before the harness can kill it
/// silently. A genuinely wedged controller therefore surfaces as a refusal with a
/// reason instead of silence.
pub const HOOK_ADMISSION_BUDGET_SECS: u64 = 90;

/// Operator override for [`HOOK_ADMISSION_BUDGET_SECS`], in whole seconds.
const HOOK_ADMISSION_BUDGET_ENV: &str = "AGENT_DOC_PREFLIGHT_HOOK_BUDGET_SECS";

fn hook_admission_budget() -> std::time::Duration {
    resolve_hook_admission_budget(std::env::var(HOOK_ADMISSION_BUDGET_ENV).ok().as_deref())
}

/// Pure half of [`hook_admission_budget`]: an unset, unparsable, or zero
/// override falls back to the default budget.
///
/// The parse lives apart from the ambient read so tests never depend on the
/// process environment. agent-doc's own dogfooding supervisor exports
/// `AGENT_DOC_PREFLIGHT_HOOK_BUDGET_SECS`, so a test that read the real env
/// went red for the operators most likely to run the suite.
fn resolve_hook_admission_budget(raw: Option<&str>) -> std::time::Duration {
    let secs = raw
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(HOOK_ADMISSION_BUDGET_SECS);
    std::time::Duration::from_secs(secs)
}

/// Seconds reserved for the binary to print its own refusal before the harness
/// deadline. Emitting the failure marker is a single `println!`, so this only
/// has to cover process scheduling.
const HOOK_REPORT_HEADROOM_SECS: u64 = 5;

/// Floor for a clamped budget. Below this the clamp would refuse turns a
/// healthy controller could still admit, which is worse than the silence.
const HOOK_MIN_CLAMPED_BUDGET_SECS: u64 = 10;

/// Fit the admission budget under the deadline the harness will actually
/// enforce (`#hookcontractlost`, submodule half).
///
/// [`HOOK_ADMISSION_BUDGET_SECS`] is chosen to sit under the
/// [`crate::skill::PREFLIGHT_HOOK_TIMEOUT_SECS`] this binary installs — but only
/// the settings file that wired *this* hook decides the real deadline, and a
/// checkout written by an older binary wires none, which means Claude's 30s
/// default. A 90s budget under a 30s deadline is not a budget: the harness kills
/// the hook and discards its output at 30s, so the binary never reaches the line
/// that would have named the overrun, and the agent sees the unwired-hook shape.
///
/// Clamping restores the invariant on the CURRENT session, without waiting for a
/// settings repair to be picked up by a restart: the binary always gets to speak
/// first. A missing installed timeout (the hook is wired in another settings
/// layer) leaves the configured budget alone rather than guessing a deadline.
fn admission_budget_under_harness_deadline(
    configured: std::time::Duration,
    installed_timeout_secs: Option<u64>,
) -> std::time::Duration {
    let Some(deadline) = installed_timeout_secs else {
        return configured;
    };
    let ceiling = deadline
        .saturating_sub(HOOK_REPORT_HEADROOM_SECS)
        .max(HOOK_MIN_CLAMPED_BUDGET_SECS);
    configured.min(std::time::Duration::from_secs(ceiling))
}

fn read_stdin_payload() -> anyhow::Result<String> {
    use std::io::Read;

    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    Ok(payload)
}

/// Emit one harness-valid UserPromptSubmit JSON response.
///
/// Codex parses stdout as JSON whenever its first non-whitespace byte is `[` or
/// `{`. Both agent-doc outcome markers begin with `[agent-doc]`, and successful
/// admission also starts with the preflight JSON object, so printing the context
/// directly makes Codex parse either one marker-shaped pseudo-array or two
/// concatenated JSON documents. Always serialize the context inside the
/// supported hook envelope instead (`#codex-hook-json-boundary`). Claude Code
/// accepts the same envelope.
fn emit_user_prompt_submit_context(context: &str) {
    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": context,
        }
    });
    println!("{output}");
}

/// Emit a machine-readable admission failure as injected turn context.
///
/// stdout only, on purpose — see [`ADMISSION_FAILURE_MARKER`]. Callers log their
/// own stderr diagnostic for the operator's hook log, because the stderr copy is
/// wanted even for prompts that never reach this function.
fn emit_admission_failure(target: &str, err: &anyhow::Error) {
    emit_user_prompt_submit_context(&format!(
        "{ADMISSION_FAILURE_MARKER}\n\
         document: {target}\n\
         reason: {err:#}\n\
         remedy: preflight refused to admit this turn, so no cycle contract exists. \
         Do NOT shell `agent-doc preflight` to recreate admission. Report this failure \
         and its reason to the operator, and stop."
    ));
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

fn run_preflight_for_prompt(
    prompt: &str,
    cwd: &Path,
    admitted_directive: Option<&str>,
) -> HookAdmission {
    let Some(invocation) = invoked_agent_doc(prompt) else {
        return HookAdmission::NotATrigger;
    };
    let target = invocation.document.clone();
    // `#hooktriggerunresolved`: this used to collapse into `NotATrigger`, which
    // is silence — and silence is what the whole `#hookcontractlost` design
    // exists to prevent. `invoked_document` already matched, so the prompt IS an
    // `agent-doc <FILE>` trigger; only the PATH failed to resolve. Reporting
    // that as "not a trigger" makes a real admission failure indistinguishable
    // from an unrelated prompt, and the agent is told to treat the missing
    // contract as a harness defect it cannot diagnose.
    //
    // Observed 2026-08-09: a `/loop agent-doc tasks/agent-doc/agent-doc-bugs2.md`
    // turn produced NO marker of either kind and no preflight activity at all.
    // A repo-relative target resolves against the hook's cwd, so any cwd that is
    // not the project root silently un-triggers the hook.
    //
    // The branch below already states the rule for the other failure path: "A
    // trigger that reached preflight must never produce silence". The same is
    // true one line earlier.
    let Some(file) = resolve_document(cwd, &target) else {
        let err = anyhow::anyhow!(
            "`{target}` did not resolve to a file from cwd `{}`. A relative document path \
             resolves against the hook's working directory; re-run from the project root, or \
             invoke the trigger with an absolute path.",
            cwd.display()
        );
        eprintln!("[agent-doc] preflight hook failed: {err:#}");
        emit_admission_failure(&target, &err);
        return HookAdmission::Failed;
    };

    // `/loop` is itself the authoritative loop-admission transition. Claiming
    // here makes the continuation lease a binary-owned effect, rather than a
    // preceding shell command the model can forget. The claim must precede
    // preflight because preflight reconciles the prior clean closeout's pending
    // continuation and classifies a missing lease as a queue stall.
    // `#hookcontractlost`: learn the deadline the harness will enforce on THIS
    // hook before spending it, and repair the settings so the next session gets
    // the full one. A checkout whose `.claude/settings.json` predates the
    // installed timeout otherwise keeps Claude's 30s default indefinitely — the
    // merge that writes it only runs on an explicit install in that directory.
    let budget = admission_budget_under_harness_deadline(
        hook_admission_budget(),
        repair_and_report_hook_deadline(cwd),
    );

    if let Err(err) = claim_loop_drain_owner(&invocation, &file) {
        let err = err.context("claim Claude loop drain-owner lease");
        eprintln!("[agent-doc] preflight hook failed: {err:#}");
        emit_admission_failure(&file.display().to_string(), &err);
        return HookAdmission::Failed;
    }

    match run_preflight_within_budget(&file, budget) {
        Ok(contract) => {
            // The marker seals a successfully produced contract. It must not
            // appear on any error path because the skill treats its absence as
            // failed admission.
            let mut context = format!("{}\n{CONTRACT_MARKER}", contract.trim_end());
            if let Some(directive) = admitted_directive {
                context.push('\n');
                context.push_str(directive);
            }
            emit_user_prompt_submit_context(&context);
            HookAdmission::Admitted
        }
        Err(err) => {
            // A trigger that reached preflight must never produce silence: the
            // agent cannot tell an absent hook from a refusing one, and the
            // refusal is the only place the remedy is known.
            eprintln!("[agent-doc] preflight hook failed: {err:#}");
            emit_admission_failure(&file.display().to_string(), &err);
            HookAdmission::Failed
        }
    }
}

/// Report the preflight deadline the Claude settings for this session wire, and
/// repair a stale one in passing.
///
/// The repaired value only reaches Claude on its next settings load, so the
/// return value is the deadline still in force for THIS turn — the repair fixes
/// the next session, the clamp fixes this one. Best-effort by construction: a
/// hook must never block an ordinary prompt over a settings file, so a failure
/// is reported to the operator's hook log and treated as an unknown deadline.
fn repair_and_report_hook_deadline(cwd: &Path) -> Option<u64> {
    // Claude reads project settings from the directory it was launched in; the
    // project root is checked too so a session started in a subdirectory still
    // finds the file that wired the hook.
    let mut candidates = vec![cwd.join(".claude/settings.json")];
    if let Some(root) = agent_doc_fs::find_project_root(cwd) {
        let rooted = root.join(".claude/settings.json");
        if !candidates.contains(&rooted) {
            candidates.push(rooted);
        }
    }
    let mut deadline = None;
    for path in candidates {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(installed) = crate::skill::installed_preflight_hook_timeout_secs(&content) else {
            continue;
        };
        // The nearest settings layer that wires the hook is the one in force.
        deadline.get_or_insert(installed);
        match crate::skill::repair_claude_preflight_hook_timeout(&path) {
            Ok(Some(previous)) => eprintln!(
                "[agent-doc] repaired {} preflight hook timeout {previous}s -> {}s; \
                 the {previous}s deadline still applies until Claude Code reloads settings",
                path.display(),
                crate::skill::PREFLIGHT_HOOK_TIMEOUT_SECS,
            ),
            Ok(None) => {}
            Err(err) => eprintln!(
                "[agent-doc] could not repair {} preflight hook timeout: {err:#}",
                path.display()
            ),
        }
    }
    deadline
}

fn claim_loop_drain_owner(invocation: &AgentDocInvocation, file: &Path) -> anyhow::Result<()> {
    if !invocation.loop_wrapped {
        return Ok(());
    }
    agent_doc_queue_io::drain_owner::refresh_drain_owner_lease(
        &file.to_string_lossy(),
        agent_doc_queue_io::drain_owner::DRAIN_OWNER_CLAUDE_LOOP,
    )
}

/// Run preflight, converting a budget overrun into a named refusal.
///
/// The worker is left running when the budget expires — preflight has no
/// cancellation point, and stopping it midway is not the goal. The goal is that
/// the *agent* learns why no contract arrived. The process exits immediately
/// after the caller reports the failure, so any cycle the worker opened is left
/// as a stale `preflight_started` that the next turn's recovery closes, which is
/// the same state a harness-side kill produced — except now with a reason
/// attached (`#hookcontractlost`).
///
/// Ordering is safe by construction: [`CONTRACT_MARKER`] is printed by the caller
/// only after preflight returns, so a worker that finishes mid-report can emit a
/// truncated contract but never the seal, and the agent's three-state read still
/// lands on "admission failed".
fn run_preflight_within_budget(file: &Path, budget: std::time::Duration) -> anyhow::Result<String> {
    let preflight_file = file.to_path_buf();
    run_within_budget(file, budget, move || {
        let mut output = Vec::new();
        agent_doc_preflight_command_io::run_with_options_to_writer(
            &preflight_file,
            agent_doc_preflight_command_io::PreflightOptions { probe: false },
            &mut output,
        )?;
        String::from_utf8(output).map_err(anyhow::Error::from)
    })
}

/// [`run_preflight_within_budget`] with the work injected.
///
/// Split out so the budget-expiry path can be tested against a worker that
/// provably does not finish. Driving it with a tiny budget and real preflight
/// instead is a race — a fast-failing preflight wins and the test asserts the
/// wrong branch (CI caught exactly that).
fn run_within_budget<T, F>(file: &Path, budget: std::time::Duration, work: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("agent-doc-preflight-hook".to_string())
        .spawn(move || {
            let outcome = work();
            // The receiver is gone on a budget overrun; the send failing there is
            // the expected shape, not a swallowed error.
            let _send_after_overrun = tx.send(outcome.map_err(|err| format!("{err:#}")));
        })?;

    match rx.recv_timeout(budget) {
        Ok(Ok(value)) => {
            // Join only on the path where the worker already finished, so a
            // slow preflight can never block past the budget.
            worker.join().ok();
            Ok(value)
        }
        Ok(Err(message)) => {
            worker.join().ok();
            Err(anyhow::anyhow!(message))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(anyhow::anyhow!(
            "preflight exceeded the hook's {}s admission budget for {} and was abandoned. \
             This is usually a wedged project controller or supervisor rather than document size; \
             check `agent-doc admin inspect` and `.agent-doc/logs/ops.log`. \
             Override the budget with {}=<seconds>.",
            budget.as_secs(),
            file.display(),
            HOOK_ADMISSION_BUDGET_ENV
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(anyhow::anyhow!(
            "preflight worker terminated without reporting an outcome for {}",
            file.display()
        )),
    }
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
    let input =
        match serde_json::from_str::<agent_doc_codex_hook_io::UserPromptSubmitInput>(&payload) {
            Ok(input) => input,
            Err(err) => {
                eprintln!("[agent-doc] preflight hook JSON parse failed: {err}");
                return Ok(());
            }
        };
    let cwd = PathBuf::from(&input.cwd);

    // Claude's Stop hook needs the same exact-session document binding as
    // Codex. Persist it before admission so a cycle is never opened without a
    // Stop boundary capable of finding the document again.
    if let Err(err) = agent_doc_codex_hook_io::apply_user_prompt_submit(&input) {
        eprintln!("[agent-doc] Claude session tracking failed: {err:#}");
        if invoked_document(&input.prompt).is_some() {
            emit_admission_failure(&input.cwd, &err.context("Claude session tracking failed"));
        }
        return Ok(());
    }

    // Every outcome is already reported to the agent and the operator inside
    // `run_preflight_for_prompt`, so a refusal never blocks an ordinary prompt.
    run_preflight_for_prompt(&input.prompt, &cwd, None);
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
    if invoked_document(&input.prompt).is_some() {
        // A process timeout must retain refusal, never reuse an older admission.
        agent_doc_codex_hook_io::record_preflight_admission(&input, false)?;
    }
    let admission = run_preflight_for_prompt(
        &input.prompt,
        Path::new(&input.cwd),
        Some(CODEX_IN_PANE_ADMISSION_DIRECTIVE),
    );
    if admission == HookAdmission::Admitted {
        agent_doc_codex_hook_io::record_preflight_admission(&input, true)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        HOOK_ADMISSION_BUDGET_SECS, admission_budget_under_harness_deadline,
        repair_and_report_hook_deadline,
    };
    use std::time::Duration;

    #[test]
    fn an_untimed_installed_hook_clamps_the_budget_under_claudes_own_deadline() {
        // `#hookcontractlost`, submodule half. A checkout wired by a binary that
        // predates the installed timeout gets Claude's 30s default, and Claude
        // DISCARDS the hook's output on expiry. A 90s budget under a 30s
        // deadline means the binary is killed before it can name its overrun, so
        // the agent sees the same silence as an unwired hook — which is exactly
        // what stalled a live `src/haiven-dev` session on 2026-09-11.
        let configured = Duration::from_secs(HOOK_ADMISSION_BUDGET_SECS);
        let clamped = admission_budget_under_harness_deadline(
            configured,
            Some(crate::skill::CLAUDE_DEFAULT_HOOK_TIMEOUT_SECS),
        );
        assert!(
            clamped < Duration::from_secs(crate::skill::CLAUDE_DEFAULT_HOOK_TIMEOUT_SECS),
            "the budget must leave room to print the refusal before the harness kills the hook, \
             got {clamped:?}"
        );
        assert_eq!(clamped, Duration::from_secs(25));
    }

    #[test]
    fn a_correctly_installed_hook_keeps_the_configured_budget() {
        let configured = Duration::from_secs(HOOK_ADMISSION_BUDGET_SECS);
        assert_eq!(
            admission_budget_under_harness_deadline(
                configured,
                Some(crate::skill::PREFLIGHT_HOOK_TIMEOUT_SECS)
            ),
            configured,
            "the installed 120s timeout already exceeds the budget; clamping must be a no-op"
        );
        assert_eq!(
            admission_budget_under_harness_deadline(configured, None),
            configured,
            "a hook wired in another settings layer has no known deadline to clamp against"
        );
        // A pathologically small deadline must not refuse turns a healthy
        // controller could still admit.
        assert_eq!(
            admission_budget_under_harness_deadline(configured, Some(2)),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn a_session_repairs_the_stale_hook_timeout_it_ran_under() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        // The exact shape a pre-timeout binary left in `src/haiven-dev`.
        std::fs::write(
            &settings,
            serde_json::to_string_pretty(&serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [{
                        "hooks": [
                            { "type": "command", "command": "agent-doc turn-status active" },
                            { "type": "command", "command": "agent-doc hook preflight-user-prompt-submit" }
                        ]
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        // The deadline reported is the one still in force for THIS turn, not the
        // repaired value — Claude has not reloaded settings yet.
        assert_eq!(
            repair_and_report_hook_deadline(dir.path()),
            Some(crate::skill::CLAUDE_DEFAULT_HOOK_TIMEOUT_SECS)
        );

        let repaired = std::fs::read_to_string(&settings).unwrap();
        assert_eq!(
            crate::skill::installed_preflight_hook_timeout_secs(&repaired),
            Some(crate::skill::PREFLIGHT_HOOK_TIMEOUT_SECS),
            "the next session must get the full timeout"
        );
        assert!(
            repaired.contains("agent-doc turn-status active"),
            "the repair must not disturb the operator's other hooks"
        );
        // Idempotent: a second pass reports the repaired deadline and rewrites nothing.
        assert_eq!(
            repair_and_report_hook_deadline(dir.path()),
            Some(crate::skill::PREFLIGHT_HOOK_TIMEOUT_SECS)
        );
    }

    #[test]
    fn a_deliberately_larger_operator_timeout_is_never_lowered() {
        // Raise-only. A bigger deadline strengthens the `#hookcontractlost`
        // invariant, so the repair must leave it alone; only a deadline below
        // the default can produce the silent output-discard.
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            serde_json::to_string_pretty(&serde_json::json!({
                "hooks": { "UserPromptSubmit": [{ "hooks": [{
                    "type": "command",
                    "command": "agent-doc hook preflight-user-prompt-submit",
                    "timeout": 300
                }]}]}
            }))
            .unwrap(),
        )
        .unwrap();

        assert_eq!(repair_and_report_hook_deadline(dir.path()), Some(300));
        assert_eq!(
            crate::skill::installed_preflight_hook_timeout_secs(
                &std::fs::read_to_string(&settings).unwrap()
            ),
            Some(300),
            "a larger operator-chosen deadline must survive the repair"
        );
    }

    #[test]
    fn a_settings_file_that_does_not_wire_preflight_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        let original = serde_json::to_string_pretty(&serde_json::json!({
            "hooks": { "UserPromptSubmit": [{ "hooks": [
                { "type": "command", "command": "agent-doc turn-status active" }
            ]}]}
        }))
        .unwrap();
        std::fs::write(&settings, &original).unwrap();

        assert_eq!(repair_and_report_hook_deadline(dir.path()), None);
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            original,
            "the hook must never add a hook a settings file does not already wire"
        );
    }


    /// `#hooktriggerunresolved`: a trigger whose document path does not resolve
    /// must FAIL LOUDLY, not read as an unrelated prompt.
    ///
    /// Both were `NotATrigger`, i.e. total silence — the exact state
    /// `#hookcontractlost` exists to prevent, and the one the agent is told to
    /// treat as an unfixable harness defect. Observed 2026-08-09: a
    /// `/loop agent-doc tasks/agent-doc/agent-doc-bugs2.md` turn produced no
    /// marker of either kind and no preflight activity at all.
    #[test]
    fn a_trigger_whose_path_does_not_resolve_fails_loudly() {
        let dir = tempfile::tempdir().unwrap();
        // The prompt IS a trigger; only the path is wrong for this cwd.
        assert_eq!(
            run_preflight_for_prompt("/loop agent-doc tasks/missing.md", dir.path(), None),
            HookAdmission::Failed,
            "an unresolvable trigger must be a named failure, never silence"
        );

        // A genuinely unrelated prompt is still a silent no-op — the hook must
        // not start shouting about every prompt in the session.
        assert_eq!(
            run_preflight_for_prompt("what does this function do?", dir.path(), None),
            HookAdmission::NotATrigger,
        );
        assert_eq!(
            run_preflight_for_prompt("/loop check the deploy", dir.path(), None),
            HookAdmission::NotATrigger,
        );
    }

    #[test]
    fn loop_admission_claims_lease_before_preflight() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, "# Session\n").unwrap();
        let invocation = invoked_agent_doc("/loop agent-doc task.md").unwrap();

        claim_loop_drain_owner(&invocation, &file).unwrap();

        let lease =
            agent_doc_queue_io::drain_owner::read_drain_owner_lease(&file.to_string_lossy())
                .expect("loop admission must hold a drain-owner lease");
        assert_eq!(
            lease.owner,
            agent_doc_queue_io::drain_owner::DRAIN_OWNER_CLAUDE_LOOP
        );
    }
    use super::*;

    /// `#hookcontractlost`: a preflight that outruns its budget must produce a
    /// reason, not silence. Without this the harness kills the hook and discards
    /// its output, leaving the agent unable to tell a slow hook from an unwired
    /// one — the exact state the admission-failure marker exists to remove.
    #[test]
    fn budget_overrun_reports_a_reason_instead_of_hanging() {
        // The worker must provably not finish, or a fast-failing one wins the
        // race and this asserts the completion branch instead of the overrun.
        let release = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_release = std::sync::Arc::clone(&release);

        let err = run_within_budget(
            Path::new("/nonexistent/agent-doc-budget-probe.md"),
            std::time::Duration::from_millis(50),
            move || {
                while !worker_release.load(std::sync::atomic::Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Ok(())
            },
        )
        .expect_err("a worker that outlives the budget cannot admit");
        release.store(true, std::sync::atomic::Ordering::SeqCst);

        let message = format!("{err:#}");
        assert!(
            message.contains("admission budget"),
            "overrun must name the budget: {message}"
        );
        assert!(
            message.contains(HOOK_ADMISSION_BUDGET_ENV),
            "overrun must name the override: {message}"
        );
    }

    /// The complementary branch: a worker that finishes inside its budget must
    /// admit, so the guard cannot refuse an ordinary fast preflight.
    #[test]
    fn work_that_finishes_inside_the_budget_admits() {
        run_within_budget(
            Path::new("/nonexistent/agent-doc-budget-probe.md"),
            std::time::Duration::from_secs(30),
            || Ok(()),
        )
        .expect("work completing inside its budget must admit");
    }

    /// A failing preflight must surface its own reason, not be relabelled as a
    /// budget overrun.
    #[test]
    fn work_that_fails_inside_the_budget_keeps_its_own_reason() {
        let err = run_within_budget(
            Path::new("/nonexistent/agent-doc-budget-probe.md"),
            std::time::Duration::from_secs(30),
            || Err::<(), _>(anyhow::anyhow!("preflight refused for its own reason")),
        )
        .expect_err("a failing worker must fail the admission");

        let message = format!("{err:#}");
        assert!(
            message.contains("preflight refused for its own reason"),
            "{message}"
        );
        assert!(
            !message.contains("admission budget"),
            "a real refusal must not be relabelled as an overrun: {message}"
        );
    }

    #[test]
    fn admission_budget_honours_the_operator_override() {
        // The parse half is exercised through the pure resolver: setting a
        // process-wide env var would race the rest of the suite, and reading
        // the real one made this test fail whenever the operator (or agent-doc's
        // own supervisor) had the override exported.
        for unusable in [None, Some(""), Some("  "), Some("not-a-number"), Some("0")] {
            assert_eq!(
                resolve_hook_admission_budget(unusable),
                std::time::Duration::from_secs(HOOK_ADMISSION_BUDGET_SECS),
                "the default budget applies when the override is unset, unparsable, or zero: {unusable:?}"
            );
        }
        assert_eq!(
            resolve_hook_admission_budget(Some(" 300 ")),
            std::time::Duration::from_secs(300),
            "a usable override replaces the default budget"
        );
    }

    /// The failure marker must stay distinguishable from the success seal: the
    /// skill greps for the seal, so an overrun that embedded it would read as an
    /// admitted cycle.
    #[test]
    fn admission_failure_marker_is_not_the_contract_marker() {
        assert_ne!(ADMISSION_FAILURE_MARKER, CONTRACT_MARKER);
        assert!(!ADMISSION_FAILURE_MARKER.contains(CONTRACT_MARKER));
    }

    #[test]
    fn codex_in_pane_directive_forbids_already_active_deflection() {
        assert!(CODEX_IN_PANE_ADMISSION_DIRECTIVE.contains("execute its unresolved work now"));
        assert!(CODEX_IN_PANE_ADMISSION_DIRECTIVE.contains("already active"));
        assert!(CODEX_IN_PANE_ADMISSION_DIRECTIVE.contains("do not"));
    }
}

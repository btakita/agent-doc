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

    match run_preflight_within_budget(&file, hook_admission_budget()) {
        Ok(()) => {
            // The marker seals a successfully produced contract. It must not
            // appear on any error path because the skill treats its absence as
            // failed admission.
            println!("{CONTRACT_MARKER}");
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
fn run_preflight_within_budget(file: &Path, budget: std::time::Duration) -> anyhow::Result<()> {
    let preflight_file = file.to_path_buf();
    run_within_budget(file, budget, move || {
        agent_doc_preflight_command_io::run_with_options(
            &preflight_file,
            agent_doc_preflight_command_io::PreflightOptions { probe: false },
        )
    })
}

/// [`run_preflight_within_budget`] with the work injected.
///
/// Split out so the budget-expiry path can be tested against a worker that
/// provably does not finish. Driving it with a tiny budget and real preflight
/// instead is a race — a fast-failing preflight wins and the test asserts the
/// wrong branch (CI caught exactly that).
fn run_within_budget<F>(file: &Path, budget: std::time::Duration, work: F) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()> + Send + 'static,
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
        Ok(Ok(())) => {
            // Join only on the path where the worker already finished, so a
            // slow preflight can never block past the budget.
            worker.join().ok();
            Ok(())
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

#[cfg(test)]
mod tests {

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
            run_preflight_for_prompt("/loop agent-doc tasks/missing.md", dir.path()),
            HookAdmission::Failed,
            "an unresolvable trigger must be a named failure, never silence"
        );

        // A genuinely unrelated prompt is still a silent no-op — the hook must
        // not start shouting about every prompt in the session.
        assert_eq!(
            run_preflight_for_prompt("what does this function do?", dir.path()),
            HookAdmission::NotATrigger,
        );
        assert_eq!(
            run_preflight_for_prompt("/loop check the deploy", dir.path()),
            HookAdmission::NotATrigger,
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
            || Err(anyhow::anyhow!("preflight refused for its own reason")),
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
}

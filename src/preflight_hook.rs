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

/// Would a session-tracking failure on this prompt leave the agent in silence?
///
/// `#hooksilentonmangledarg`: two different parsers read the prompt in
/// [`handle_user_prompt_submit`]. Session tracking binds its document with the
/// permissive `resolve_agent_doc_path`; the admission guard asked the strict
/// `invoked_document`, which refuses a second positional argument. A prompt the
/// permissive parser accepted and the strict one rejected — a mistyped or
/// doubled `FILE` argument is the shape seen in the wild — therefore failed
/// tracking loudly on stderr and emitted NEITHER stdout marker, which
/// `SKILL.md` tells the agent to read as "the hook never ran": a harness-wiring
/// defect it cannot diagnose, rather than the one-line path typo it is.
///
/// Observed 2026-09-12: `/agent-doc <path>.md/agent-doc <path>.md` (a doubled
/// slash-command argument) canonicalized `<path>.md/agent-doc`, failed with
/// `Not a directory (os error 20)`, and exited 0 with no contract of either
/// kind.
///
/// The union is deliberate: this only ever *adds* a named refusal on a path
/// that already failed, and never suppresses one. Trigger recognition for
/// admission itself is untouched — the strict parser still owns that.
fn tracking_recognized_trigger(prompt: &str) -> bool {
    invoked_document(prompt).is_some()
        || agent_doc_codex_hook_io::prompt_names_agent_doc_document(prompt)
}

/// Emit a machine-readable admission failure as injected turn context.
///
/// stdout only, on purpose — see [`ADMISSION_FAILURE_MARKER`]. Callers log their
/// own stderr diagnostic for the operator's hook log, because the stderr copy is
/// wanted even for prompts that never reach this function.
fn emit_admission_failure(target: &str, err: &anyhow::Error) {
    emit_user_prompt_submit_context(&admission_failure_payload_for_error(target, err));
}

/// The refusal payload for `err`: a preflight the admission deadline stopped
/// (`#preflightdeadline`) gets a remedy that says the refusal is retryable;
/// every other failure keeps the generic remedy.
fn admission_failure_payload_for_error(target: &str, err: &anyhow::Error) -> String {
    let reason = format!("{err:#}");
    match err.downcast_ref::<agent_doc_preflight_command_io::progress::PreflightAdmissionRefused>()
    {
        Some(refusal) => admission_deadline_refusal_payload(target, &reason, refusal.phase()),
        None => admission_failure_payload(target, &reason),
    }
}

/// `#preflightdeadline`: the payload for a run the admission deadline stopped.
///
/// Same marker, `document:`, `reason:` and `pending:` lines as every refusal,
/// so the agent's three-state read is unchanged; only the remedy differs. The
/// run stopped at a step boundary (or at a wait clamped to the deadline), not
/// at a verdict, so re-triggering is the recovery — the generic remedy's
/// "report and stop" would wrongly read as terminal.
fn admission_deadline_refusal_payload(target: &str, reason: &str, phase: &str) -> String {
    format!(
        "{ADMISSION_FAILURE_MARKER}\n\
         document: {target}\n\
         reason: {reason}\n\
         pending: operator steering may be waiting unanswered -- this refused turn read no \
         document changes. `agent-doc session-check {target}` is a permitted follow-up: it lists \
         any unreconciled operator prompt verbatim without starting a response.\n\
         remedy: retryable -- the preflight admission deadline stopped this run at phase \
         `{phase}`, before the hook budget could abandon it mid-step; no controller or document \
         refused this turn. Re-send `agent-doc {target}` to retry admission: the next preflight \
         starts afresh, and a `preflight_started` cycle this run opened is closed by its \
         recovery. Do NOT shell `agent-doc preflight` to recreate admission, and do not start a \
         response or write the document this turn. Tell the operator admission timed out in \
         phase `{phase}` and can be retried, run `agent-doc session-check {target}` and relay \
         any pending operator prompt it lists, then stop. If retries keep stopping at `{phase}`, \
         that phase is the work to move off the admission path, or raise the budget with \
         {HOOK_ADMISSION_BUDGET_ENV}=<seconds>."
    )
}

/// `#refusalsteering` (GH #71): the refusal payload. A refused turn is the one
/// path where preflight consults no steering surface, so an operator prompt
/// already typed into `exchange` sat unanswered until the operator asked. The
/// refusal still fails closed on writes, but it now says steering may be
/// pending, names `session-check` as the permitted follow-up that lists it, and
/// -- when the reason is a queue/CRDT reconciliation -- tells the agent to warn
/// the operator that their latest edit may not have been received yet.
fn admission_failure_payload(target: &str, reason: &str) -> String {
    let reconciling = [
        "reconciliation is pending",
        "retry_crdt_merge",
        "queue authority is unavailable",
        "retained",
    ]
    .iter()
    .any(|needle| reason.contains(needle));
    let edit_note = if reconciling {
        " The reason is an unsettled queue/CRDT reconciliation, so the operator's most recent \
         edit may be inside the save that could not be merged: tell them it may not have been \
         received yet and to re-trigger once it settles."
    } else {
        ""
    };
    format!(
        "{ADMISSION_FAILURE_MARKER}\n\
         document: {target}\n\
         reason: {reason}\n\
         pending: operator steering may be waiting unanswered -- this refused turn read no \
         document changes. `agent-doc session-check {target}` is a permitted follow-up: it lists \
         any unreconciled operator prompt verbatim -- including one still inside an unmerged editor \
         save (`#refusalsteeringverbatim`) -- without starting a response.{edit_note}\n\
         remedy: preflight refused to admit this turn, so no cycle contract exists. \
         Do NOT shell `agent-doc preflight` to recreate admission, and do not start a response \
         or write the document. Report this failure and its reason to the operator, run \
         `agent-doc session-check {target}` and relay any pending operator prompt it lists, \
         then stop."
    )
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
    preflight_invocation: agent_doc_preflight_command_io::PreflightInvocation,
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
        // `#pastetriggeradmit`: a multi-line prompt that merely BEGINS with the
        // trigger text (a pasted transcript whose first line was
        // `agent-doc tasks/fpe.md`, from another project root) is operator
        // content. The supported "trigger plus prompt body" shape always names
        // a document that resolves, so an unresolved multi-line prompt is left
        // alone instead of being reported as a refused admission. A single-line
        // trigger still fails loudly (`#hooktriggerunresolved`).
        if agent_doc_prompt_contract::harness_prompt::is_multi_line_prompt(prompt) {
            eprintln!(
                "[agent-doc] preflight hook: multi-line prompt begins with `agent-doc {target}` \
                 but that document does not resolve from cwd `{}`; treating it as pasted \
                 content, not a trigger",
                cwd.display()
            );
            return HookAdmission::NotATrigger;
        }
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

    match run_preflight_admission(&file, budget, preflight_invocation) {
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

/// `#admissiontransportretry` — how much of the admission budget must remain for
/// a second attempt to be worth starting.
///
/// Below this there is not enough time for preflight to finish, and spending what
/// is left would trade a named refusal for an unnamed overrun — the
/// `#hookcontractlost` failure this hook exists to prevent.
const ADMISSION_RETRY_MIN_REMAINING: std::time::Duration = std::time::Duration::from_secs(20);

/// What to do with a failed admission attempt (`#admissiontransportretry`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdmissionRetry {
    /// Re-ask with the remaining budget.
    Retry,
    /// The controller authored this refusal; it is final.
    ControllerRefusal,
    /// A transport drop, but too little budget remains to finish a second
    /// attempt. Report the named refusal instead of risking a silent overrun.
    InsufficientBudget,
}

/// Pure half of [`run_preflight_admission`]: classify a failed attempt.
///
/// Split out because both decisions are exactly the ones worth pinning — that a
/// controller-authored refusal is never retried, and that a transport drop is
/// retried only with enough budget left to finish — and neither should need a
/// live controller to test.
fn classify_admission_failure(
    err: &anyhow::Error,
    remaining: std::time::Duration,
) -> AdmissionRetry {
    if !agent_doc_controller_io::project_controller::controller_transport_drop_is_retryable(err) {
        return AdmissionRetry::ControllerRefusal;
    }
    if remaining < ADMISSION_RETRY_MIN_REMAINING {
        return AdmissionRetry::InsufficientBudget;
    }
    AdmissionRetry::Retry
}

/// Run preflight admission, re-asking once if the controller transport dropped.
///
/// A controller that is recycled or replaced by a newer build mid-request closes
/// the connection without answering, and the client surfaces `project controller
/// closed connection without a response`. That is a transport fact, not a
/// verdict: the controller never said this turn may not proceed. Surfacing it as
/// a terminal refusal kills an operator's `agent-doc <FILE>` for a half-second of
/// controller churn — observed 2026-09-27 on `tasks/software/lazily.md`, where the
/// agent correctly reported the refusal and stopped, because a refusal with a
/// reason is exactly what it is told to trust.
///
/// `agent_doc_controller_io::project_controller` already classifies this message
/// as retryable and already retries it for `coordination_claim`, `actor_binding`,
/// and `dispatch`. Admission is the one boundary where *not* retrying costs a
/// whole turn, so it re-asks with the same predicate — never a second spelling of
/// the message.
///
/// Re-asking is safe here because it is the ordinary path: preflight runs afresh
/// every turn, and a `preflight_started` cycle left stale-empty by the dropped
/// attempt auto-closes under the interrupted-cycle guard. Only the transport drop
/// retries — a refusal the controller authored is final and must stay final.
fn run_preflight_admission(
    file: &Path,
    budget: std::time::Duration,
    preflight_invocation: agent_doc_preflight_command_io::PreflightInvocation,
) -> anyhow::Result<String> {
    let started = std::time::Instant::now();
    let first = run_preflight_within_budget(file, budget, preflight_invocation);
    let Err(err) = first else {
        return first;
    };
    let remaining = budget.saturating_sub(started.elapsed());
    match classify_admission_failure(&err, remaining) {
        AdmissionRetry::ControllerRefusal => return Err(err),
        AdmissionRetry::InsufficientBudget => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_admission_transport_drop_not_retried file={} remaining_ms={} \
                     min_remaining_ms={} reason=insufficient_budget (#admissiontransportretry)",
                    file.display(),
                    remaining.as_millis(),
                    ADMISSION_RETRY_MIN_REMAINING.as_millis(),
                ),
            );
            return Err(err);
        }
        AdmissionRetry::Retry => {}
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "preflight_admission_transport_drop_retry file={} remaining_ms={} \
             reason=stale_or_recycled_controller detail={} (#admissiontransportretry)",
            file.display(),
            remaining.as_millis(),
            agent_doc_secret_redact::redact(&format!("{err:#}")).replace('\n', " "),
        ),
    );
    eprintln!(
        "[agent-doc] preflight admission: controller transport dropped ({err:#}); re-asking once within the remaining {:.1}s budget",
        remaining.as_secs_f32(),
    );
    run_preflight_within_budget(file, remaining, preflight_invocation)
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
fn run_preflight_within_budget(
    file: &Path,
    budget: std::time::Duration,
    invocation: agent_doc_preflight_command_io::PreflightInvocation,
) -> anyhow::Result<String> {
    let preflight_file = file.to_path_buf();
    run_within_budget(file, budget, move || {
        let mut output = Vec::new();
        agent_doc_preflight_command_io::run_with_options_to_writer(
            &preflight_file,
            agent_doc_preflight_command_io::PreflightOptions {
                probe: false,
                invocation,
            },
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
    // `#preflightdeadline`: the run carries an admission deadline (the budget
    // minus a margin). Installing it clamps every bounded wait beneath preflight
    // to the time remaining, and each phase boundary refuses by name once it has
    // passed, so the backstop timeout below is reached only by a step that
    // blocks outside every clamped wait.
    let progress =
        agent_doc_preflight_command_io::progress::PreflightProgress::with_admission_deadline(
            budget,
        );
    let worker_progress = progress.clone();
    let worker = std::thread::Builder::new()
        .name("agent-doc-preflight-hook".to_string())
        .spawn(move || {
            let _phases =
                agent_doc_preflight_command_io::progress::install(worker_progress.clone());
            // Classify before `_phases` drops, while the running phase is recorded.
            let outcome = work().map_err(|err| worker_progress.refuse_failed_run(err));
            // The receiver is gone on a budget overrun; the send failing there is
            // the expected shape, not a swallowed error.
            let _send_after_overrun = tx.send(outcome);
        })?;

    match rx.recv_timeout(budget) {
        Ok(Ok(value)) => {
            // Join only on the path where the worker already finished, so a
            // slow preflight can never block past the budget.
            worker.join().ok();
            Ok(value)
        }
        Ok(Err(err)) => {
            worker.join().ok();
            if let Some(refusal) = err
                .downcast_ref::<agent_doc_preflight_command_io::progress::PreflightAdmissionRefused>()
            {
                report_deadline_refusal(file, refusal);
            }
            Err(err)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let snapshot = progress.snapshot();
            report_overrun_phases(file, budget, &snapshot);
            Err(anyhow::anyhow!(overrun_reason(file, budget, &snapshot)))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(anyhow::anyhow!(
            "preflight worker terminated without reporting an outcome for {}",
            file.display()
        )),
    }
}

/// Where the preflight was when the admission budget expired (GH #78).
///
/// The phase still running is the measured cause; it replaces the fixed guess
/// ("usually a wedged project controller or supervisor") this refusal used to
/// carry, which an observed overrun with a ready controller had already refuted.
fn overrun_phase_clause(
    snapshot: &agent_doc_preflight_command_io::progress::ProgressSnapshot,
) -> String {
    let completed = if snapshot.completed.is_empty() {
        String::new()
    } else {
        format!(
            "; completed phases, costliest first: {}",
            snapshot.breakdown()
        )
    };
    match snapshot.running {
        Some((label, running)) => format!(
            "It was still in preflight phase `{label}` ({}ms in that phase) when the budget expired{completed}.",
            running.as_millis()
        ),
        None if snapshot.completed.is_empty() => {
            "Preflight recorded no phase before the budget expired: it never reached its first step."
                .to_string()
        }
        None => format!("Preflight had left every instrumented phase{completed}."),
    }
}

/// The refusal reason for a budget overrun.
///
/// It names the measured phase, an `admin inspect` invocation that is accepted
/// as written (the bare form is rejected without a target), and what happened
/// to the abandoned worker: it ends when this hook process exits, so it cannot
/// race the next trigger.
fn overrun_reason(
    file: &Path,
    budget: std::time::Duration,
    snapshot: &agent_doc_preflight_command_io::progress::ProgressSnapshot,
) -> String {
    format!(
        "preflight exceeded the hook's {}s admission budget for {} and was abandoned. {} \
         Check `agent-doc admin inspect {}` and that phase in `.agent-doc/logs/ops.log`. \
         The abandoned preflight worker stops when this hook process exits, so it cannot race \
         the next trigger; a `preflight_started` cycle it opened is closed by the next turn's \
         recovery. Override the budget with {}=<seconds>.",
        budget.as_secs(),
        file.display(),
        overrun_phase_clause(snapshot),
        file.display(),
        HOOK_ADMISSION_BUDGET_ENV
    )
}

/// Record the overrun's phase breakdown where an operator reads after the fact,
/// in the same `[perf]` shape as `session_check.operations`.
fn report_overrun_phases(
    file: &Path,
    budget: std::time::Duration,
    snapshot: &agent_doc_preflight_command_io::progress::ProgressSnapshot,
) {
    let running = snapshot
        .running
        .map(|(label, running)| format!("{label}:{}ms", running.as_millis()))
        .unwrap_or_else(|| "-".to_string());
    let line = format!(
        "preflight_admission_overrun file={} budget_ms={} elapsed_ms={} running={} completed={} (#preflightoverrunphase)",
        file.display(),
        budget.as_millis(),
        snapshot.elapsed.as_millis(),
        running,
        if snapshot.completed.is_empty() {
            "-".to_string()
        } else {
            snapshot.breakdown()
        },
    );
    eprintln!("[perf] {line}");
    agent_doc_ops_log_io::log_op(file, &line);
}

/// Record a deadline refusal where an operator reads after the fact. The phase
/// it names is the evidence `#preflightdeadline` step 3 waits for: the work to
/// move off the admission path.
fn report_deadline_refusal(
    file: &Path,
    refusal: &agent_doc_preflight_command_io::progress::PreflightAdmissionRefused,
) {
    use agent_doc_preflight_command_io::progress::RefusalPoint;
    let point = match refusal.point {
        RefusalPoint::BeforePhase(_) => "before_phase",
        RefusalPoint::DuringPhase(_) => "during_phase",
    };
    let line = format!(
        "preflight_admission_deadline_refused file={} phase={} point={} elapsed_ms={} \
         deadline_ms={} budget_ms={} previous={} completed={} (#preflightdeadline)",
        file.display(),
        refusal.phase(),
        point,
        refusal.elapsed.as_millis(),
        refusal.deadline_after.as_millis(),
        refusal.budget.as_millis(),
        refusal
            .previous
            .map(|(label, took)| format!("{label}:{}ms", took.as_millis()))
            .unwrap_or_else(|| "-".to_string()),
        if refusal.completed.is_empty() {
            "-"
        } else {
            refusal.completed.as_str()
        },
    );
    eprintln!("[perf] {line}");
    agent_doc_ops_log_io::log_op(file, &line);
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
        if tracking_recognized_trigger(&input.prompt) {
            emit_admission_failure(&input.cwd, &err.context("Claude session tracking failed"));
        }
        return Ok(());
    }

    // Every outcome is already reported to the agent and the operator inside
    // `run_preflight_for_prompt`, so a refusal never blocks an ordinary prompt.
    run_preflight_for_prompt(
        &input.prompt,
        &cwd,
        None,
        agent_doc_preflight_command_io::PreflightInvocation::ClaudeCodeHook,
    );
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

    // Codex may deliver a second identical trigger as real-time steering into
    // the turn whose hook already admitted this document. Preserve that
    // admission before `apply_user_prompt_submit` replaces its receipt with a
    // fresh pending one. Exact turn/prompt/document identity keeps unrelated
    // and genuinely new prompts on the normal fail-closed path.
    if let Some(existing) = agent_doc_codex_hook_io::same_turn_admitted_invocation(&input)? {
        let reuse = serde_json::to_string_pretty(&serde_json::json!({
            "reused_admission": true,
            "kind": "same_turn_repeat",
            "document": existing.doc_path,
            "cycle_id": existing.cycle_id,
            "required_action": "Continue the unresolved work in the current turn; the original cycle contract remains authoritative.",
        }))?;
        emit_user_prompt_submit_context(&format!(
            "{reuse}\n{CONTRACT_MARKER}\n{CODEX_IN_PANE_ADMISSION_DIRECTIVE}"
        ));
        return Ok(());
    }

    if let Err(err) = agent_doc_codex_hook_io::apply_user_prompt_submit(&input) {
        eprintln!("[agent-doc] Codex session tracking failed: {err:#}");
        // Tracking failed, so preflight must not open a cycle the Stop hook
        // cannot find. When this prompt *was* an `agent-doc <FILE>` trigger the
        // agent would otherwise see silence, so name the refusal
        // (`#hookcontractlost`). Ordinary prompts stay untouched.
        if tracking_recognized_trigger(&input.prompt) {
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
        agent_doc_preflight_command_io::PreflightInvocation::CodexHook,
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
            run_preflight_for_prompt(
                "/loop agent-doc tasks/missing.md",
                dir.path(),
                None,
                agent_doc_preflight_command_io::PreflightInvocation::ClaudeCodeHook,
            ),
            HookAdmission::Failed,
            "an unresolvable trigger must be a named failure, never silence"
        );

        // A genuinely unrelated prompt is still a silent no-op — the hook must
        // not start shouting about every prompt in the session.
        assert_eq!(
            run_preflight_for_prompt(
                "what does this function do?",
                dir.path(),
                None,
                agent_doc_preflight_command_io::PreflightInvocation::ClaudeCodeHook,
            ),
            HookAdmission::NotATrigger,
        );
        assert_eq!(
            run_preflight_for_prompt(
                "/loop check the deploy",
                dir.path(),
                None,
                agent_doc_preflight_command_io::PreflightInvocation::ClaudeCodeHook,
            ),
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

        // 200ms budget => 150ms admission deadline, comfortably after both
        // phase boundaries below, so this exercises the backstop (a step that
        // blocks outside every clamped wait), not the deadline refusal.
        let err = run_within_budget(
            Path::new("/nonexistent/agent-doc-budget-probe.md"),
            std::time::Duration::from_millis(200),
            move || {
                agent_doc_preflight_command_io::progress::enter("resolve_initial_document")?;
                std::thread::sleep(std::time::Duration::from_millis(2));
                agent_doc_preflight_command_io::progress::enter("settle_debounce")?;
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
        // GH #78: the cause is measured, not asserted.
        assert!(
            message.contains("still in preflight phase `settle_debounce`"),
            "overrun must name the phase still running: {message}"
        );
        assert!(
            message.contains("resolve_initial_document:"),
            "overrun must list completed phases: {message}"
        );
        assert!(
            !message.contains("usually a wedged"),
            "the fixed guess must not survive a measured phase: {message}"
        );
        // GH #78: bare `agent-doc admin inspect` is rejected without a target.
        assert!(
            message.contains("`agent-doc admin inspect /nonexistent/agent-doc-budget-probe.md`"),
            "the remedy must name an accepted inspect invocation: {message}"
        );
        assert!(
            message.contains("stops when this hook process exits"),
            "the refusal must say what happens to the abandoned worker: {message}"
        );
    }

    /// `#preflightdeadline` (2): a run that crosses its admission deadline
    /// between phases stops at that boundary with the typed refusal naming the
    /// phase — before the backstop could abandon it — and the hook maps that
    /// refusal to the UNAVAILABLE output with a reason and a retryable remedy.
    #[test]
    fn crossing_the_deadline_at_a_phase_boundary_refuses_by_name_before_the_backstop() {
        use agent_doc_preflight_command_io::progress::{PreflightAdmissionRefused, RefusalPoint};
        // 2s budget => 1.5s deadline: the 1.6s phase crosses it, and the
        // refusal lands ~0.4s before the backstop would have fired.
        let started = std::time::Instant::now();
        let err = run_within_budget(
            Path::new("/nonexistent/agent-doc-deadline-probe.md"),
            std::time::Duration::from_secs(2),
            || -> anyhow::Result<()> {
                agent_doc_preflight_command_io::progress::enter("commit_previous_cycle")?;
                std::thread::sleep(std::time::Duration::from_millis(1600));
                agent_doc_preflight_command_io::progress::enter("settle_debounce")?;
                panic!("the phase after the deadline must never start");
            },
        )
        .expect_err("a run past its deadline cannot admit");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));

        let refusal = err
            .downcast_ref::<PreflightAdmissionRefused>()
            .unwrap_or_else(|| panic!("the refusal must stay typed: {err:#}"));
        assert_eq!(refusal.point, RefusalPoint::BeforePhase("settle_debounce"));

        let payload =
            admission_failure_payload_for_error("/nonexistent/agent-doc-deadline-probe.md", &err);
        assert!(payload.starts_with(ADMISSION_FAILURE_MARKER), "{payload}");
        assert!(
            payload.contains(
                "reason: preflight admission deadline reached at the boundary before phase \
                 `settle_debounce`"
            ),
            "{payload}"
        );
        assert!(
            payload.contains("the last phase, `commit_previous_cycle`"),
            "{payload}"
        );
        assert!(
            payload.contains("remedy: retryable") && payload.contains("phase `settle_debounce`"),
            "{payload}"
        );
        assert!(
            payload.contains("Do NOT shell `agent-doc preflight`"),
            "{payload}"
        );
        assert!(
            !payload.contains("was abandoned"),
            "a boundary refusal is not the backstop overrun: {payload}"
        );
        assert!(!payload.contains(CONTRACT_MARKER), "{payload}");
    }

    /// `#preflightdeadline` (1): a bounded wait inside a phase is clamped to
    /// the time remaining, so a 30s local bound ends at the deadline, and the
    /// error it ends with becomes the refusal naming the running phase.
    #[test]
    fn a_bounded_wait_is_clamped_to_the_deadline_and_refuses_in_its_phase() {
        use agent_doc_preflight_command_io::progress::{PreflightAdmissionRefused, RefusalPoint};
        let started = std::time::Instant::now();
        let err = run_within_budget(
            Path::new("/nonexistent/agent-doc-deadline-probe.md"),
            std::time::Duration::from_millis(800),
            || {
                agent_doc_preflight_command_io::progress::enter("pre_mutation_debounce")?;
                let bound = agent_doc_debounce::admission_deadline::clamp(
                    std::time::Duration::from_secs(30),
                );
                assert!(bound <= std::time::Duration::from_millis(600), "{bound:?}");
                std::thread::sleep(bound);
                Err::<(), _>(anyhow::anyhow!(
                    "preflight deferred: Lazily current authority remained delivery_pending"
                ))
            },
        )
        .expect_err("a wait that hit the deadline cannot admit");
        assert!(started.elapsed() < std::time::Duration::from_millis(800));
        let refusal = err
            .downcast_ref::<PreflightAdmissionRefused>()
            .unwrap_or_else(|| panic!("the refusal must stay typed: {err:#}"));
        assert_eq!(
            refusal.point,
            RefusalPoint::DuringPhase("pre_mutation_debounce")
        );
        let message = format!("{err:#}");
        assert!(
            message.contains("delivery_pending"),
            "the cause is kept: {message}"
        );
    }

    /// A preflight that never reached its first step says so instead of naming
    /// a phase it was not in.
    #[test]
    fn overrun_before_any_phase_says_no_phase_was_reached() {
        let snapshot =
            agent_doc_preflight_command_io::progress::PreflightProgress::new().snapshot();
        let clause = overrun_phase_clause(&snapshot);
        assert!(clause.contains("never reached its first step"), "{clause}");
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
    fn admission_failure_payload_points_at_pending_steering() {
        let reason = "preflight refused admission: retained queue reconciliation is pending; \
                      recovery=retry_crdt_merge";
        let payload = admission_failure_payload("tasks/doc.md", reason);
        assert!(payload.starts_with(ADMISSION_FAILURE_MARKER), "{payload}");
        assert!(
            payload.contains("pending: operator steering may be waiting"),
            "{payload}"
        );
        assert!(
            payload.contains("agent-doc session-check tasks/doc.md"),
            "{payload}"
        );
        assert!(
            payload.contains("may not have been received yet"),
            "{payload}"
        );
        assert!(
            payload.contains("Do NOT shell `agent-doc preflight`"),
            "{payload}"
        );

        let other = admission_failure_payload("tasks/doc.md", "no project root found");
        assert!(
            other.contains("agent-doc session-check tasks/doc.md"),
            "{other}"
        );
        assert!(!other.contains("may not have been received yet"), "{other}");
    }

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

    /// `#admissiontransportretry` — the message a recycled/replaced controller
    /// leaves behind. Observed 2026-09-27 ending an `agent-doc
    /// tasks/software/lazily.md` turn as if the controller had refused it.
    #[test]
    fn a_controller_transport_drop_earns_a_second_admission_attempt() {
        let err = anyhow::anyhow!("project controller closed connection without a response");
        assert_eq!(
            classify_admission_failure(&err, HOOK_ADMISSION_BUDGET_SECS_DURATION),
            AdmissionRetry::Retry
        );
    }

    /// A refusal the controller AUTHORED is a verdict, not a transport fact, and
    /// re-asking it would be the retry loop this hook must never become.
    #[test]
    fn a_controller_authored_refusal_is_never_retried() {
        for message in [
            "controller not authoritative",
            "admission_divergence: refusing to choose a winner",
            "`plan.md` did not resolve to a file",
        ] {
            let err = anyhow::anyhow!(message);
            assert_eq!(
                classify_admission_failure(&err, HOOK_ADMISSION_BUDGET_SECS_DURATION),
                AdmissionRetry::ControllerRefusal,
                "{message}"
            );
        }
    }

    /// With the budget nearly spent, a second attempt cannot finish — and an
    /// unnamed overrun is worse than a named refusal (`#hookcontractlost`).
    #[test]
    fn a_transport_drop_with_no_budget_left_keeps_the_named_refusal() {
        let err = anyhow::anyhow!("project controller closed connection without a response");
        assert_eq!(
            classify_admission_failure(&err, std::time::Duration::from_secs(1)),
            AdmissionRetry::InsufficientBudget
        );
        // The boundary itself: exactly the minimum retries, one below does not.
        assert_eq!(
            classify_admission_failure(&err, ADMISSION_RETRY_MIN_REMAINING),
            AdmissionRetry::Retry
        );
        assert_eq!(
            classify_admission_failure(
                &err,
                ADMISSION_RETRY_MIN_REMAINING - std::time::Duration::from_millis(1)
            ),
            AdmissionRetry::InsufficientBudget
        );
    }

    const HOOK_ADMISSION_BUDGET_SECS_DURATION: std::time::Duration =
        std::time::Duration::from_secs(HOOK_ADMISSION_BUDGET_SECS);
}

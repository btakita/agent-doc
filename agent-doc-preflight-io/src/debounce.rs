use agent_doc_crdt_relay_io::CrdtReplicaEventReason;
use agent_doc_debounce::{SettleAction, SettleBudget, SettleDeferReason, SettleTimers};
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

/// Poll cadence inherited from the document's configured debounce budget.
/// Debounce is no longer an editor-authority signal; Lazily current state is.
pub fn authority_settle_ms(file: &Path) -> u64 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|content| {
            agent_doc_frontmatter::parse(&content)
                .ok()
                .and_then(|(fm, _)| fm.debounce_ms)
        })
        .unwrap_or(2000)
}

/// One Lazily current-transition observation, in the shape the shared settle
/// decision consumes.
struct Observation {
    ready: bool,
    state: &'static str,
    /// `Some(live_editors)` when the transition is `delivery_pending` and an
    /// urgent CRDT delivery drain can be requested against those replicas.
    drain_targets: Option<usize>,
    /// Observed current text, used as positive evidence that the frontier is
    /// still advancing (`#routeprogresswait`).
    text: Option<String>,
    error: Option<String>,
}

fn observe_lazily_current(file: &Path, source: &str) -> Observation {
    use agent_doc_crdt_relay_io::CurrentText;

    let ready = |state| Observation {
        ready: true,
        state,
        drain_targets: None,
        text: None,
        error: None,
    };
    let pending = |state, drain_targets, text| Observation {
        ready: false,
        state,
        drain_targets,
        text,
        error: None,
    };

    match agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
        file, source,
    ) {
        Ok(None | Some(CurrentText::Detached)) => ready("detached"),
        Ok(Some(CurrentText::Current {
            delivery_converged: true,
            ..
        })) => ready("lazily_current"),
        Ok(Some(CurrentText::Current {
            text, live_editors, ..
        })) => pending("delivery_pending", Some(live_editors), Some(text)),
        Ok(Some(CurrentText::EditorAttachedMissingReplica)) => {
            pending("missing_replica", None, None)
        }
        Ok(Some(CurrentText::EditorSyncPending)) => pending("current_pending", None, None),
        Err(error) => Observation {
            ready: false,
            state: "authority_unavailable",
            drain_targets: None,
            text: None,
            error: Some(error.to_string()),
        },
    }
}

/// Serialize a visible mutation behind Lazily's current-authority transition.
///
/// This deliberately does not infer operator activity from a filesystem typing
/// marker or disk mtime. The coherent current document is the authority, and the
/// eventual mutation remains guarded by its expected-current CAS.
///
/// `#preflightsettleparity`: this wait drives the same `agent_doc_debounce`
/// settle decision as the route startup wait, so it inherits both mitigations
/// route already had. Without them, a `Run Agent Doc` dispatch that cleared the
/// route wait was still killed here: preflight polled a `delivery_pending`
/// frontier that nobody drained (`#crdtpushdrain`) and hard-failed on wall clock
/// even while the frontier was converging (`#routeprogresswait`), which aborts
/// the whole preflight and reads to the operator as `Run Agent Doc` doing
/// nothing.
pub fn wait_for_lazily_current_before_mutation(file: &Path) -> Result<()> {
    wait_for_lazily_current_before_mutation_with_effects(
        file,
        agent_doc_debounce::authority_settle_max_wait(authority_settle_ms(file)),
        observe_lazily_current,
        agent_doc_crdt_relay_io::signal_crdt_replica_event,
    )
}

fn wait_for_lazily_current_before_mutation_with_effects<Observe, Signal>(
    file: &Path,
    max_wait: std::time::Duration,
    mut observe: Observe,
    mut signal: Signal,
) -> Result<()>
where
    Observe: FnMut(&Path, &str) -> Observation,
    Signal: FnMut(&Path, CrdtReplicaEventReason, usize) -> Result<()>,
{
    let poll = agent_doc_debounce::SETTLE_POLL_INTERVAL;
    // `#preflightdeadline`: the settle window is clamped to the preflight
    // admission deadline, so this wait can never be the one that outlives it.
    let budget = SettleBudget::from_no_progress(max_wait).clamped_to_admission_deadline();
    let start = Instant::now();
    let mut last_progress = Instant::now();
    let mut last_observed: Option<String> = None;
    let mut last_urgent_drain: Option<Instant> = None;
    let mut reregister_rounds = 0u32;

    loop {
        let observation = observe(file, "preflight_visible_mutation");

        // `#preflightdeadline`: the clamped budget bounds one settle window, but
        // the re-register and progress branches below grant further windows.
        // Once the admission deadline is spent, a still-pending transition ends
        // this wait with the typed refusal instead of another window.
        if !observation.ready
            && let Err(exhausted) = agent_doc_debounce::admission_deadline::ensure_remaining(
                "preflight_visible_mutation_settle",
            )
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_visible_mutation_admission_deadline file={} state={} waited_ms={} (#preflightdeadline)",
                    file.display(),
                    observation.state,
                    start.elapsed().as_millis(),
                ),
            );
            return Err(exhausted.into());
        }

        if observation.text.is_some() && observation.text != last_observed {
            if last_observed.is_some() {
                last_progress = Instant::now();
            }
            last_observed = observation.text.clone();
        }

        let timers = SettleTimers {
            stalled_for: last_progress.elapsed(),
            total_elapsed: start.elapsed(),
            since_last_urgent_drain: last_urgent_drain.map(|last| last.elapsed()),
        };
        match agent_doc_debounce::settle_step(
            observation.ready,
            observation.drain_targets.is_some(),
            timers,
            budget,
        ) {
            SettleAction::Ready => return Ok(()),
            SettleAction::Wait {
                request_urgent_drain,
            } => {
                if let Some(targets) = observation.drain_targets.filter(|_| request_urgent_drain) {
                    let reason = CrdtReplicaEventReason::CanonicalProjection;
                    last_urgent_drain = Some(Instant::now());
                    if let Err(error) = signal(file, reason, targets) {
                        eprintln!(
                            "[preflight] urgent CRDT delivery drain request failed (reason={} targets={targets} error={error:#})",
                            reason.token()
                        );
                    }
                }
            }
            SettleAction::Defer { reason } => {
                // The delivery frontier can converge between the observation above and
                // this deadline decision. Failing from the stale sample turns a healthy
                // final ACK into an operator-visible preflight error. Re-read the
                // authority exactly once at the defer boundary; this stays fail-closed
                // for a genuinely stalled frontier while admitting convergence that
                // already happened.
                let boundary_observation =
                    observe(file, "preflight_visible_mutation_defer_boundary");
                if boundary_observation.ready {
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "preflight_visible_mutation_defer_boundary_converged file={} prior_state={} final_state={}",
                            file.display(),
                            observation.state,
                            boundary_observation.state,
                        ),
                    );
                    return Ok(());
                }
                if reason == SettleDeferReason::NoProgress
                    && boundary_observation.text.is_some()
                    && boundary_observation.text != last_observed
                {
                    last_progress = Instant::now();
                    last_observed = boundary_observation.text.clone();
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "preflight_visible_mutation_defer_boundary_progress file={} state={}",
                            file.display(),
                            boundary_observation.state,
                        ),
                    );
                    wait_one_settle_slice(file, boundary_observation.state, poll);
                    continue;
                }
                // `#rundocdispatchrobust`: a missing editor model is recoverable, not
                // terminal. The editor is open (the relay routes it as attached) but its
                // replica is not registered, e.g. a dynamic plugin reload lost the new
                // generation's liveness report and the controller refused its
                // registrations as a stale endpoint. Ask that editor to re-register and
                // give it another no-progress window, a bounded number of times, instead
                // of refusing the operator's Run Agent Doc on the first 3.7s window.
                if reason == SettleDeferReason::NoProgress
                    && editor_model_missing_state(boundary_observation.state)
                    && reregister_rounds < PREFLIGHT_EDITOR_REREGISTER_ROUNDS
                {
                    reregister_rounds += 1;
                    let status =
                        match signal(file, CrdtReplicaEventReason::EditorReplicaReregister, 0) {
                            Ok(()) => "requested".to_string(),
                            Err(error) => format!("failed:{error:#}").replace('\n', " "),
                        };
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "preflight_visible_mutation_editor_reregister_requested file={} state={} round={}/{} reregister={}",
                            file.display(),
                            boundary_observation.state,
                            reregister_rounds,
                            PREFLIGHT_EDITOR_REREGISTER_ROUNDS,
                            status,
                        ),
                    );
                    last_progress = Instant::now();
                    wait_one_settle_slice(file, boundary_observation.state, poll);
                    continue;
                }
                let waited = match reason {
                    SettleDeferReason::NoProgress => timers.stalled_for,
                    SettleDeferReason::ProgressCeiling => timers.total_elapsed,
                };
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "preflight_visible_mutation_deferred_lazily_current file={} state={} timeout_ms={} error={}",
                        file.display(),
                        boundary_observation.state,
                        waited.as_millis(),
                        boundary_observation.error.as_deref().unwrap_or("none")
                    ),
                );
                anyhow::bail!(
                    "preflight deferred for {}: Lazily current authority remained {} for {}ms; retry after the current transition settles{}",
                    file.display(),
                    boundary_observation.state,
                    waited.as_millis(),
                    boundary_observation
                        .error
                        .as_deref()
                        .map(|error| format!(" ({error})"))
                        .unwrap_or_default()
                );
            }
        }
        wait_one_settle_slice(file, observation.state, poll);
    }
}

/// `#rundocdispatchrobust`: how many extra no-progress windows preflight grants an
/// attached editor whose model is missing, each after asking it to re-register.
const PREFLIGHT_EDITOR_REREGISTER_ROUNDS: u32 = 3;

/// Whether `state` means an attached editor's replica is not registered, which an
/// editor re-registration repairs. `authority_unavailable` covers the controller's
/// bounded ensure giving up on `editor_attached_model_missing`.
fn editor_model_missing_state(state: &str) -> bool {
    matches!(state, "missing_replica" | "authority_unavailable")
}

/// Longest single park while delivery is settling.
///
/// The settle loop's own cadence is [`agent_doc_debounce::SETTLE_POLL_INTERVAL`]
/// (100ms), and every tick costs a controller round trip that recomputes the whole
/// current text. When the blocker is `delivery_pending` that tick is pure waste: the
/// controller can tell us the moment delivery converges, so one park replaces the
/// spin. Kept short enough that urgent-drain and progress sampling still happen on
/// roughly their old cadence.
const DELIVERY_SETTLE_AWAIT_SLICE: std::time::Duration = std::time::Duration::from_millis(500);

/// Wait one slice of the settle loop.
///
/// `#lazily-hot-path` Theme A (W3): when the blocker is specifically a settling
/// delivery, park on the controller's delivery-convergence await instead of sleeping
/// blind. It returns the instant convergence lands — so a settle that would have been
/// noticed up to a poll-interval late is noticed immediately — and it collapses the
/// ~10 observations per second this loop otherwise makes into one park.
///
/// Every other blocker (missing replica, current pending, authority unavailable) has
/// no convergence fact to wait on, so it keeps the plain sleep. So does an await that
/// cannot answer (no hub for the document, or no reachable controller): falling back
/// to the old cadence keeps this strictly fail-open, since the loop's budget and
/// deferral semantics are unchanged either way.
fn wait_one_settle_slice(file: &Path, state: &str, poll: std::time::Duration) {
    // `#preflightdeadline`: no slice parks past the admission deadline.
    let poll = agent_doc_debounce::admission_deadline::clamp(poll);
    let await_slice = agent_doc_debounce::admission_deadline::clamp(DELIVERY_SETTLE_AWAIT_SLICE);
    if state != "delivery_pending" || await_slice.is_zero() {
        std::thread::sleep(poll);
        return;
    }
    match agent_doc_controller_io::project_controller::await_delivery_convergence_for_file(
        file,
        await_slice,
    ) {
        // Observed: the await already consumed up to its slice, returning early only
        // when convergence landed. Loop straight back to the authoritative observation.
        Ok(Some(_)) => {}
        Ok(None) => std::thread::sleep(poll),
        Err(error) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_settle_convergence_unavailable file={} fallback=poll_interval detail={}",
                    file.display(),
                    format!("{error:#}")
                        .replace('\n', " | ")
                        .chars()
                        .take(160)
                        .collect::<String>()
                ),
            );
            std::thread::sleep(poll);
        }
    }
}

/// Observe a coherent Lazily current cut before preflight reads the document.
/// Mutation sites use [`wait_for_lazily_current_before_mutation`] and fail closed;
/// this read-only observation remains bounded and lets later CAS checks decide.
pub fn wait_for_lazily_current_observation(file: &Path) {
    let settle_ms = authority_settle_ms(file);
    // `#preflightdeadline`: an observation wait never outlives the admission
    // deadline; the next phase boundary then refuses by name.
    let max_wait = agent_doc_debounce::admission_deadline::clamp(
        agent_doc_debounce::authority_settle_max_wait(settle_ms),
    );
    let start = std::time::Instant::now();

    loop {
        let observation = observe_lazily_current(file, "preflight_observation");
        if observation.ready {
            tracing::debug!(
                waited_ms = start.elapsed().as_millis() as u64,
                authority_state = observation.state,
                file = %file.display(),
                "preflight Lazily current observed"
            );
            return;
        }
        if start.elapsed() >= max_wait {
            tracing::warn!(
                waited_ms = start.elapsed().as_millis() as u64,
                authority_state = observation.state,
                error = observation.error.as_deref().unwrap_or("none"),
                "preflight Lazily current observation timeout; later expected-current CAS remains authoritative"
            );
            return;
        }
        tracing::trace!(
            authority_state = observation.state,
            "preflight Lazily current pending"
        );
        std::thread::sleep(agent_doc_debounce::admission_deadline::clamp(
            agent_doc_debounce::SETTLE_POLL_INTERVAL,
        ));
    }
}

/// How an operator-edit quiescence wait ended (`#qheadcomposing`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorEditQuiescence {
    /// The coherent current text still equals what preflight admitted: no
    /// operator edit landed during preflight, so no wait was spent.
    Unchanged,
    /// No coherent current text could be observed (detached / pending); the
    /// earlier Lazily-current observation owns that state.
    Unobservable,
    /// The text changed during preflight and then stayed unchanged for the
    /// quiet window.
    Settled { waited: Duration, changes: usize },
    /// The text kept changing until the bounded ceiling; preflight proceeds.
    CeilingReached { waited: Duration, changes: usize },
}

/// `#qheadcomposing`: wait for an operator who is still typing to pause before
/// preflight computes the diff and selects the queue head.
///
/// A converged Lazily current cut only proves that every keystroke delivered so
/// far is visible; it says nothing about whether the operator has finished the
/// line. Operator-reported 2026-10-03 (`tasks/software/tsift.md`): a restart
/// auto-trigger dispatched one second after the operator started typing a queue
/// item, preflight admitted `- Should we release + publish the ` (note the
/// trailing space of a word boundary just typed), and the response answered
/// that fragment while the operator finished `... the C++ bindings?`.
///
/// The evidence is the authoritative current text itself, never a filesystem
/// typing marker: when the text differs from the `admitted` read taken at the
/// start of preflight, an edit landed during preflight, so the wait requires the
/// text to stay unchanged for the document's debounce window before admitting
/// it. When nothing changed the wait returns at once, so an idle document pays
/// no latency.
pub fn wait_for_operator_edit_quiescence(file: &Path, admitted: &str) -> OperatorEditQuiescence {
    let settle_ms = authority_settle_ms(file);
    let quiet = Duration::from_millis(settle_ms);
    let ceiling = agent_doc_debounce::admission_deadline::clamp(
        agent_doc_debounce::authority_settle_max_wait(settle_ms)
            .saturating_mul(agent_doc_debounce::PROGRESS_WAIT_CEILING_MULTIPLIER),
    );
    let outcome = await_operator_edit_quiescence(
        admitted,
        quiet,
        ceiling,
        || {
            match agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
                file,
                "preflight_operator_edit_quiescence",
            ) {
                Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => Some(text),
                _ => None,
            }
        },
        Instant::now,
        |wait| std::thread::sleep(agent_doc_debounce::admission_deadline::clamp(wait)),
    );
    match &outcome {
        OperatorEditQuiescence::Settled { waited, changes }
        | OperatorEditQuiescence::CeilingReached { waited, changes } => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_operator_edit_quiescence file={} outcome={} waited_ms={} changes={} quiet_ms={} (#qheadcomposing)",
                    file.display(),
                    if matches!(outcome, OperatorEditQuiescence::Settled { .. }) {
                        "settled"
                    } else {
                        "ceiling_reached"
                    },
                    waited.as_millis(),
                    changes,
                    quiet.as_millis(),
                ),
            );
        }
        OperatorEditQuiescence::Unchanged | OperatorEditQuiescence::Unobservable => {}
    }
    outcome
}

fn await_operator_edit_quiescence<Observe, Now, Sleep>(
    admitted: &str,
    quiet: Duration,
    ceiling: Duration,
    mut observe: Observe,
    mut now: Now,
    mut sleep: Sleep,
) -> OperatorEditQuiescence
where
    Observe: FnMut() -> Option<String>,
    Now: FnMut() -> Instant,
    Sleep: FnMut(Duration),
{
    let start = now();
    let Some(mut last) = observe() else {
        return OperatorEditQuiescence::Unobservable;
    };
    if last == admitted {
        return OperatorEditQuiescence::Unchanged;
    }
    let mut changes = 1usize;
    let mut last_change = start;
    loop {
        let at = now();
        let waited = at.saturating_duration_since(start);
        if at.saturating_duration_since(last_change) >= quiet {
            return OperatorEditQuiescence::Settled { waited, changes };
        }
        if waited >= ceiling {
            return OperatorEditQuiescence::CeilingReached { waited, changes };
        }
        sleep(agent_doc_debounce::SETTLE_POLL_INTERVAL);
        if let Some(text) = observe()
            && text != last
        {
            last = text;
            last_change = now();
            changes += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn pending(text: &str, live_editors: usize) -> Observation {
        Observation {
            ready: false,
            state: "delivery_pending",
            drain_targets: Some(live_editors),
            text: Some(text.to_owned()),
            error: None,
        }
    }

    fn converged() -> Observation {
        Observation {
            ready: true,
            state: "lazily_current",
            drain_targets: None,
            text: None,
            error: None,
        }
    }

    /// `#lazily-hot-path` W3 — only a settling *delivery* has a convergence fact to
    /// park on. Every other blocker must keep the plain poll, and so must an await
    /// that cannot answer, or the loop would stall on a fact nobody will ever report.
    ///
    /// Operator-reported 2026-07-25: 20 consecutive deferrals, all
    /// `state=delivery_pending`, mean 12.1s each (242s total). At a 100ms cadence each
    /// of those burned ~120 controller round trips that each recompute the full
    /// current text.
    #[test]
    fn only_a_settling_delivery_parks_on_the_convergence_fact() {
        // No project root => the await errors immediately, exercising the fail-open
        // path without a controller. Each call must therefore return promptly and
        // never hang, whatever the blocker.
        let file = std::path::Path::new("/nonexistent-agent-doc-root/settle-probe.md");
        for state in [
            "delivery_pending",
            "missing_replica",
            "current_pending",
            "authority_unavailable",
        ] {
            let started = std::time::Instant::now();
            wait_one_settle_slice(file, state, Duration::from_millis(10));
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "settle slice for {state} must stay bounded when no controller can answer"
            );
        }
    }

    /// `#preflightsettleparity`: preflight's pre-mutation wait used to poll a
    /// `delivery_pending` frontier without ever asking anyone to drain it, so a
    /// delivery only the drain would complete burned the whole budget and
    /// aborted preflight — the operator-visible "JB `Run Agent Doc` stalls".
    #[test]
    fn preflight_mutation_wait_requests_an_urgent_delivery_drain_while_pending() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let observations = Cell::new(0usize);
        let signals = RefCell::new(Vec::new());

        wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::from_secs(1),
            |_file, _source| {
                let n = observations.get();
                observations.set(n + 1);
                if n < 3 {
                    pending("prompt", 1)
                } else {
                    converged()
                }
            },
            |_file, reason, targets| {
                signals.borrow_mut().push((reason, targets));
                Ok(())
            },
        )
        .expect("a converging delivery must settle rather than defer");

        assert_eq!(
            signals.into_inner(),
            vec![(CrdtReplicaEventReason::CanonicalProjection, 1)],
            "preflight must pull the pending delivery instead of only polling it"
        );
    }

    /// `#preflightsettleparity`: an advancing frontier must reset the
    /// no-progress deadline here exactly as it does on the route side, so a slow
    /// but healthy ACK round trip no longer hard-fails preflight.
    #[test]
    fn preflight_mutation_wait_does_not_defer_a_frontier_that_keeps_advancing() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let observations = Cell::new(0usize);

        let started = Instant::now();
        let outcome = wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::from_millis(300),
            |_file, _source| {
                let n = observations.get();
                observations.set(n + 1);
                if n < 12 {
                    pending(&format!("prompt {n}"), 1)
                } else {
                    converged()
                }
            },
            |_file, _reason, _targets| Ok(()),
        );

        assert!(
            outcome.is_ok(),
            "an advancing frontier must not be deferred: {outcome:?}"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "the fixture must actually outlast the no-progress budget"
        );
    }

    /// The progress reset is not a blank cheque: a genuinely wedged transition
    /// still fails closed with the same operator-facing reason.
    #[test]
    fn preflight_mutation_wait_still_defers_a_stalled_frontier() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");

        let outcome = wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::from_millis(300),
            |_file, _source| pending("frozen", 1),
            |_file, _reason, _targets| Ok(()),
        );

        let message = format!("{:#}", outcome.unwrap_err());
        assert!(
            message.contains("delivery_pending") && message.contains("preflight deferred"),
            "a wedged transition must still fail closed with its reason: {message}"
        );
    }

    fn missing_replica() -> Observation {
        Observation {
            ready: false,
            state: "missing_replica",
            drain_targets: None,
            text: None,
            error: None,
        }
    }

    /// `#rundocdispatchrobust`: devops.md 2026-09-29 18:14. A dynamic plugin reload
    /// left the open editor's replica unregistered, and preflight refused Run Agent
    /// Doc after one no-progress window. It now asks the editor to re-register and
    /// admits once the replica is back.
    #[test]
    fn preflight_mutation_wait_reregisters_a_missing_editor_model_instead_of_refusing() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let signals = RefCell::new(Vec::new());

        let outcome = wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::from_millis(50),
            |_file, _source| {
                if signals.borrow().is_empty() {
                    missing_replica()
                } else {
                    converged()
                }
            },
            |_file, reason, targets| {
                signals.borrow_mut().push((reason, targets));
                Ok(())
            },
        );

        assert!(
            outcome.is_ok(),
            "a re-registered editor must be admitted: {outcome:?}"
        );
        assert_eq!(
            signals.into_inner(),
            vec![(CrdtReplicaEventReason::EditorReplicaReregister, 0)]
        );
    }

    /// The re-register recovery is bounded: an editor that never comes back still
    /// fails closed with the same reason, after a fixed number of requests.
    #[test]
    fn preflight_mutation_wait_bounds_editor_reregister_recovery() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let signals = Cell::new(0u32);

        let outcome = wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::from_millis(30),
            |_file, _source| missing_replica(),
            |_file, reason, _targets| {
                assert_eq!(reason, CrdtReplicaEventReason::EditorReplicaReregister);
                signals.set(signals.get() + 1);
                Ok(())
            },
        );

        let message = format!("{:#}", outcome.unwrap_err());
        assert!(message.contains("missing_replica") && message.contains("preflight deferred"));
        assert_eq!(signals.get(), PREFLIGHT_EDITOR_REREGISTER_ROUNDS);
    }

    /// A delivery ACK can land after the last ordinary observation but before
    /// the no-progress decision is reported. The defer boundary must observe
    /// that convergence instead of failing from the stale pending sample.
    #[test]
    fn preflight_mutation_wait_accepts_convergence_at_the_defer_boundary() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let observations = Cell::new(0usize);

        let outcome = wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::ZERO,
            |_file, source| {
                let n = observations.get();
                observations.set(n + 1);
                if source == "preflight_visible_mutation_defer_boundary" {
                    converged()
                } else {
                    assert_eq!(
                        n, 0,
                        "only the initial pending sample should precede the boundary"
                    );
                    pending("frozen", 1)
                }
            },
            |_file, _reason, _targets| Ok(()),
        );

        assert!(
            outcome.is_ok(),
            "convergence already visible at the defer boundary must be admitted: {outcome:?}"
        );
        assert_eq!(observations.get(), 2);
    }

    fn deadline_in(left: Duration) -> agent_doc_debounce::admission_deadline::DeadlineGuard {
        agent_doc_debounce::admission_deadline::install(
            agent_doc_debounce::admission_deadline::AdmissionDeadline {
                at: std::time::Instant::now() + left,
                budget: Duration::from_secs(90),
            },
        )
    }

    /// `#preflightdeadline`: the settle wait's own window (here 30s, and up to
    /// three more re-register windows) is clamped to the admission deadline, and
    /// a transition still pending when it passes ends with the typed refusal.
    #[test]
    fn preflight_mutation_wait_is_clamped_to_the_admission_deadline() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let _deadline = deadline_in(Duration::from_millis(300));

        let started = std::time::Instant::now();
        let outcome = wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::from_secs(30),
            |_file, _source| missing_replica(),
            |_file, _reason, _targets| Ok(()),
        );

        let err = outcome.expect_err("a transition pending past the deadline cannot admit");
        let exhausted = err
            .downcast_ref::<agent_doc_debounce::admission_deadline::AdmissionDeadlineExhausted>()
            .unwrap_or_else(|| panic!("the refusal must stay typed: {err:#}"));
        assert_eq!(exhausted.wait, "preflight_visible_mutation_settle");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the 30s settle window must have been clamped, took {:?}",
            started.elapsed()
        );
    }

    /// A transition that is already current is admitted even with the deadline
    /// spent: the deadline bounds waiting, it never refuses a ready answer.
    #[test]
    fn preflight_mutation_wait_admits_a_ready_transition_at_a_spent_deadline() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let _deadline = deadline_in(Duration::ZERO);

        let outcome = wait_for_lazily_current_before_mutation_with_effects(
            &doc,
            Duration::from_secs(30),
            |_file, _source| converged(),
            |_file, _reason, _targets| Ok(()),
        );
        assert!(outcome.is_ok(), "{outcome:?}");
    }

    /// `#qheadcomposing` fixture: the tsift.md session document with `line` as
    /// the only queue item, as the operator was typing it on 2026-10-03.
    fn tsift_queue_doc(line: &str) -> String {
        let item = if line.is_empty() {
            String::new()
        } else {
            format!("{line}\n")
        };
        format!(
            "---\nagent_doc_session: tsift-v0.1\nagent_doc_format: template\n---\n\n\
             ## Exchange\n\n<!-- agent:exchange -->\nPrior answer.\n<!-- /agent:exchange -->\n\n\
             ## Queue\n\n<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" priority -->\n\
             {item}<!-- /agent:queue -->\n"
        )
    }

    /// Drive [`await_operator_edit_quiescence`] on a fake clock against a typing
    /// timeline of `(offset_ms, queue line)` keystroke states. Returns the
    /// outcome, the text preflight would admit, and the number of sleeps.
    fn run_typing_timeline(
        admitted: &str,
        timeline: &[(u64, &str)],
        quiet: Duration,
        ceiling: Duration,
    ) -> (OperatorEditQuiescence, String, usize) {
        let origin = Instant::now();
        let clock = Cell::new(origin);
        let last_seen = RefCell::new(String::new());
        let sleeps = Cell::new(0usize);
        let outcome = await_operator_edit_quiescence(
            admitted,
            quiet,
            ceiling,
            || {
                let elapsed = clock.get().duration_since(origin).as_millis() as u64;
                let line = timeline
                    .iter()
                    .rev()
                    .find(|(at, _)| *at <= elapsed)
                    .map(|(_, line)| *line)
                    .unwrap_or("");
                let text = tsift_queue_doc(line);
                *last_seen.borrow_mut() = text.clone();
                Some(text)
            },
            || clock.get(),
            |wait| {
                sleeps.set(sleeps.get() + 1);
                clock.set(clock.get() + wait);
            },
        );
        (outcome, last_seen.into_inner(), sleeps.get())
    }

    fn admitted_queue_head(text: &str) -> String {
        agent_doc_queue::queue_consume::next_queue_head_selection(text)
            .unwrap()
            .expect("queue head")
            .head_text
    }

    /// `#qheadcomposing` regression (operator-reported 2026-10-03, tsift.md): a
    /// restart auto-trigger dispatched while the operator was typing a queue
    /// item. Preflight read `- Should we re`, and by the diff the line was
    /// `- Should we release + publish the ` — the response quoted that fragment
    /// as its `> **Queue prompt:**`. Preflight must wait for the line to stop
    /// changing and admit the whole item.
    #[test]
    fn preflight_waits_for_a_queue_item_the_operator_is_still_typing() {
        let admitted = tsift_queue_doc("- Should we re");
        let timeline = [
            (0, "- Should we release + publish the "),
            (900, "- Should we release + publish the C"),
            (1_800, "- Should we release + publish the C++"),
            (3_400, "- Should we release + publish the C++ bindings"),
            (4_500, "- Should we release + publish the C++ bindings?"),
        ];
        let (outcome, admitted_text, _) = run_typing_timeline(
            &admitted,
            &timeline,
            Duration::from_millis(2_000),
            Duration::from_secs(18),
        );
        let OperatorEditQuiescence::Settled { waited, changes } = outcome else {
            panic!("expected the typing to settle, got {outcome:?}");
        };
        assert!(waited >= Duration::from_millis(6_500), "{waited:?}");
        assert_eq!(changes, timeline.len());
        assert_eq!(
            admitted_queue_head(&admitted_text),
            "Should we release + publish the C++ bindings?",
            "preflight must admit the finished queue item, not the fragment"
        );
    }

    #[test]
    fn preflight_pays_no_wait_when_nothing_changed_during_preflight() {
        let admitted = tsift_queue_doc("- Should we release + publish the C++ bindings?");
        let timeline = [(0, "- Should we release + publish the C++ bindings?")];
        let (outcome, _, sleeps) = run_typing_timeline(
            &admitted,
            &timeline,
            Duration::from_millis(2_000),
            Duration::from_secs(18),
        );
        assert_eq!(outcome, OperatorEditQuiescence::Unchanged);
        assert_eq!(sleeps, 0, "an idle document must not pay the quiet window");
    }

    #[test]
    fn preflight_edit_quiescence_is_bounded_when_typing_never_pauses() {
        let admitted = tsift_queue_doc("- before preflight");
        let lines: Vec<String> = (0..400).map(|n| format!("- a{}", "b".repeat(n))).collect();
        let timeline: Vec<(u64, &str)> = lines
            .iter()
            .enumerate()
            .map(|(n, line)| (n as u64 * 500, line.as_str()))
            .collect();
        let (outcome, _, _) = run_typing_timeline(
            &admitted,
            &timeline,
            Duration::from_millis(2_000),
            Duration::from_secs(5),
        );
        let OperatorEditQuiescence::CeilingReached { waited, .. } = outcome else {
            panic!("expected the ceiling to bound the wait, got {outcome:?}");
        };
        assert!(waited >= Duration::from_secs(5) && waited < Duration::from_secs(6));
    }

    #[test]
    fn preflight_edit_quiescence_defers_to_the_observation_wait_without_a_current_cut() {
        let outcome = await_operator_edit_quiescence(
            "anything",
            Duration::from_millis(2_000),
            Duration::from_secs(18),
            || None,
            Instant::now,
            |_| panic!("an unobservable document must not wait"),
        );
        assert_eq!(outcome, OperatorEditQuiescence::Unobservable);
    }
}

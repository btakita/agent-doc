use agent_doc_crdt_relay_io::CrdtReplicaEventReason;
use agent_doc_debounce::{SettleAction, SettleBudget, SettleDeferReason, SettleTimers};
use anyhow::Result;
use std::path::Path;
use std::time::Instant;

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
    let budget = SettleBudget::from_no_progress(max_wait);
    let start = Instant::now();
    let mut last_progress = Instant::now();
    let mut last_observed: Option<String> = None;
    let mut last_urgent_drain: Option<Instant> = None;

    loop {
        let observation = observe(file, "preflight_visible_mutation");

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
                let waited = match reason {
                    SettleDeferReason::NoProgress => timers.stalled_for,
                    SettleDeferReason::ProgressCeiling => timers.total_elapsed,
                };
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "preflight_visible_mutation_deferred_lazily_current file={} state={} timeout_ms={} error={}",
                        file.display(),
                        observation.state,
                        waited.as_millis(),
                        observation.error.as_deref().unwrap_or("none")
                    ),
                );
                anyhow::bail!(
                    "preflight deferred for {}: Lazily current authority remained {} for {}ms; retry after the current transition settles{}",
                    file.display(),
                    observation.state,
                    waited.as_millis(),
                    observation
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
    if state != "delivery_pending" {
        std::thread::sleep(poll);
        return;
    }
    match agent_doc_controller_io::project_controller::await_delivery_convergence_for_file(
        file,
        DELIVERY_SETTLE_AWAIT_SLICE,
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
    let max_wait = agent_doc_debounce::authority_settle_max_wait(settle_ms);
    let poll = std::time::Duration::from_millis(100);
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
        std::thread::sleep(poll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

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
}

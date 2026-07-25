//! Route startup Lazily-current transition wait.

use agent_doc_crdt_relay_io::{CrdtReplicaEventReason, CurrentText};
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

/// `#crdtpushdrain` / `#routeprogresswait` policy is shared with the preflight
/// pre-mutation wait so both paths pull a pending delivery and reset their
/// no-progress deadline on an advancing frontier. See `agent_doc_debounce`.
///
/// The JetBrains `Run Agent Doc` action writes its prompt marker into the
/// document *first* (`active_exchange_prompt_marker ... status=applied_ack_pending`),
/// so route then waits for the convergence of a write the same action just
/// issued. Under load that ACK round trip can exceed a flat `debounce * 10`
/// (5000ms), and route deferred with "Lazily current transition remained
/// delivery_pending for 5000ms" even though the frontier was converging normally.
use agent_doc_debounce::{SettleAction, SettleBudget, SettleDeferReason, SettleTimers};

#[cfg(test)]
const URGENT_DRAIN_RETRY_INTERVAL: Duration = agent_doc_debounce::URGENT_DRAIN_RETRY_INTERVAL;

/// Wait for Lazily's current-document transition to settle.
///
/// Route fails closed instead of dispatching through an incomplete current
/// transition. Disk mtime and filesystem typing markers are not authority.
pub fn await_idle(file: &Path, debounce: Duration) -> Result<()> {
    await_idle_with_max_wait(file, debounce, debounce * 10)
}

pub fn await_idle_with_max_wait(file: &Path, debounce: Duration, max_wait: Duration) -> Result<()> {
    await_idle_with_max_wait_and_effects(
        file,
        debounce,
        max_wait,
        |file, source| {
            agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
                file, source,
            )
        },
        agent_doc_crdt_relay_io::signal_crdt_replica_event,
    )
}

fn await_idle_with_max_wait_and_effects<Observe, Signal>(
    file: &Path,
    debounce: Duration,
    max_wait: Duration,
    mut observe: Observe,
    mut signal: Signal,
) -> Result<()>
where
    Observe: FnMut(&Path, &str) -> Result<Option<CurrentText>>,
    Signal: FnMut(&Path, CrdtReplicaEventReason, usize) -> Result<()>,
{
    let poll_interval = agent_doc_debounce::SETTLE_POLL_INTERVAL;
    let start = Instant::now();
    let _ = debounce;
    let mut last_urgent_drain: Option<Instant> = None;
    // `#routeprogresswait`: the no-progress deadline restarts whenever the
    // observed frontier advances, so a converging transition is not deferred on
    // wall-clock alone.
    let mut last_progress = Instant::now();
    let mut last_observed: Option<String> = None;
    let budget = SettleBudget::from_no_progress(max_wait);

    loop {
        let current = observe(file, "route_startup_current_transition");
        let observed_text = match &current {
            Ok(Some(CurrentText::Current { text, .. })) => Some(text.clone()),
            _ => None,
        };
        let (ready, state, drain_targets) = match current {
            Ok(None | Some(CurrentText::Detached)) => (true, "detached", None),
            Ok(Some(CurrentText::Current {
                delivery_converged: true,
                ..
            })) => (true, "lazily_current", None),
            Ok(Some(CurrentText::Current {
                live_editors,
                delivery_converged: false,
                ..
            })) => (false, "delivery_pending", Some(live_editors)),
            Ok(Some(CurrentText::EditorAttachedMissingReplica)) => (false, "missing_replica", None),
            Ok(Some(CurrentText::EditorSyncPending)) => (false, "current_pending", None),
            Err(_) => (false, "authority_unavailable", None),
        };
        if observed_text.is_some() && observed_text != last_observed {
            if last_observed.is_some() {
                last_progress = Instant::now();
            }
            last_observed = observed_text;
        }

        let timers = SettleTimers {
            stalled_for: last_progress.elapsed(),
            total_elapsed: start.elapsed(),
            since_last_urgent_drain: last_urgent_drain.map(|last| last.elapsed()),
        };
        match agent_doc_debounce::settle_step(ready, drain_targets.is_some(), timers, budget) {
            SettleAction::Ready => {
                eprintln!("[route] Lazily current transition settled ({state})");
                return Ok(());
            }
            SettleAction::Defer { reason } => {
                let detail = match reason {
                    SettleDeferReason::ProgressCeiling => format!(
                        "kept advancing without converging for {}ms (progress ceiling)",
                        timers.total_elapsed.as_millis()
                    ),
                    SettleDeferReason::NoProgress => {
                        format!(
                            "remained {} for {}ms",
                            state,
                            timers.stalled_for.as_millis()
                        )
                    }
                };
                anyhow::bail!(
                    "route deferred for {}: Lazily current transition {}; retry after it settles",
                    file.display(),
                    detail
                );
            }
            SettleAction::Wait {
                request_urgent_drain,
            } => {
                // #crdtpushdrain: re-request on a bounded cadence rather than once. A single
                // urgent drain can legitimately apply nothing — `drainRemoteUpdatesFor`
                // returns early while the path has pending local edits or is mid editor
                // apply, and the forwarder may not be registered yet. Its only follow-up is
                // the *gated* `requestRemoteDrain`, which an idle document's escalated no-op
                // backoff suppresses, so a one-shot latch left the remaining budget polling a
                // frontier nobody would pull and route deferred at `max_wait`.
                if let Some(targets) = drain_targets.filter(|_| request_urgent_drain) {
                    last_urgent_drain = Some(Instant::now());
                    let reason = CrdtReplicaEventReason::AckRecoveryForceRefresh;
                    match signal(file, reason, targets) {
                        Ok(()) => eprintln!(
                            "[route] requested urgent CRDT delivery drain (reason={} targets={targets})",
                            reason.token()
                        ),
                        Err(error) => eprintln!(
                            "[route] urgent CRDT delivery drain request failed (reason={} targets={targets} error={error:#})",
                            reason.token()
                        ),
                    }
                }
            }
        }

        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn route_dispatches_immediately_when_lazily_is_detached() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "settled prompt\n").unwrap();

        let start = Instant::now();
        await_idle_with_max_wait(&doc, Duration::from_millis(50), Duration::from_millis(2000))
            .expect("detached Lazily authority authorizes immediate dispatch");
        assert!(
            start.elapsed() < Duration::from_millis(50),
            "route must not impose a filesystem debounce when Lazily is detached (elapsed {:?})",
            start.elapsed()
        );
    }

    #[test]
    fn route_startup_requests_urgent_delivery_drain_before_waiting() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let observations = Cell::new(0usize);
        let signals = RefCell::new(Vec::new());

        await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            Duration::from_secs(1),
            |_file, _source| {
                let observation = observations.get();
                observations.set(observation + 1);
                if observation < 3 {
                    Ok(Some(CurrentText::Current {
                        text: "prompt".to_owned(),
                        live_editors: 1,
                        delivery_converged: false,
                    }))
                } else {
                    Ok(Some(CurrentText::Current {
                        text: "prompt".to_owned(),
                        live_editors: 1,
                        delivery_converged: true,
                    }))
                }
            },
            |_file, reason, targets| {
                signals.borrow_mut().push((reason, targets));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            signals.into_inner(),
            vec![(CrdtReplicaEventReason::AckRecoveryForceRefresh, 1)]
        );
        assert!(observations.get() >= 4);
    }

    /// `#routeprogresswait`: regression for the recurring
    /// `route deferred ...: Lazily current transition remained delivery_pending for
    /// 5000ms`. The JetBrains `Run Agent Doc` action writes its prompt marker into
    /// the document first, so route waits on the convergence of a write that same
    /// action just issued. Under load that ACK round trip can outlast a flat
    /// `debounce * 10`, and route deferred a transition that was converging
    /// normally. An advancing frontier must reset the no-progress deadline.
    #[test]
    fn route_startup_does_not_defer_a_frontier_that_keeps_advancing() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let observations = Cell::new(0usize);

        let started = Instant::now();
        let outcome = await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            // A no-progress budget far shorter than the total time this
            // transition takes: without progress tracking it would defer.
            Duration::from_millis(300),
            |_file, _source| {
                let n = observations.get();
                observations.set(n + 1);
                if n < 12 {
                    // Text advances on every poll — actively converging.
                    Ok(Some(CurrentText::Current {
                        text: format!("prompt {n}"),
                        live_editors: 1,
                        delivery_converged: false,
                    }))
                } else {
                    Ok(Some(CurrentText::Current {
                        text: "prompt final".to_owned(),
                        live_editors: 1,
                        delivery_converged: true,
                    }))
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

    /// The progress reset is not a blank cheque: a frontier that is genuinely
    /// stuck (identical text every poll) still defers at the no-progress budget.
    #[test]
    fn route_startup_still_defers_a_stalled_frontier() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");

        let outcome = await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            Duration::from_millis(300),
            |_file, _source| {
                Ok(Some(CurrentText::Current {
                    text: "frozen".to_owned(),
                    live_editors: 1,
                    delivery_converged: false,
                }))
            },
            |_file, _reason, _targets| Ok(()),
        );

        assert!(
            outcome.is_err(),
            "a stalled frontier must still fail closed"
        );
        let message = format!("{:#}", outcome.unwrap_err());
        assert!(
            message.contains("delivery_pending"),
            "the stall reason should still be reported: {message}"
        );
    }

    /// `#crdtpushdrain`: regression for the reported
    /// `route deferred ...: Lazily current transition remained delivery_pending for
    /// 5000ms`. A single urgent drain can apply nothing (pending local edits, a
    /// mid-apply path, an unregistered forwarder), and its only follow-up is the
    /// *gated* drain that an idle document's escalated no-op backoff suppresses. A
    /// one-shot latch therefore burned the whole budget on a frontier nobody pulled.
    #[test]
    fn route_startup_retries_urgent_delivery_drain_while_delivery_stays_pending() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let signals = RefCell::new(Vec::new());

        let outcome = await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            URGENT_DRAIN_RETRY_INTERVAL * 3,
            |_file, _source| {
                Ok(Some(CurrentText::Current {
                    text: "prompt".to_owned(),
                    live_editors: 2,
                    delivery_converged: false,
                }))
            },
            |_file, reason, targets| {
                signals.borrow_mut().push((reason, targets));
                Ok(())
            },
        );

        assert!(
            outcome.is_err(),
            "a never-converging delivery must still fail closed at max_wait"
        );
        let signals = signals.into_inner();
        assert!(
            signals.len() >= 3,
            "route must keep re-requesting the urgent drain while delivery is pending, \
             not latch after one attempt (got {} request(s))",
            signals.len()
        );
        assert!(
            signals.iter().all(|(reason, targets)| *reason
                == CrdtReplicaEventReason::AckRecoveryForceRefresh
                && *targets == 2),
            "every retry must carry the same force-refresh reason and live target count"
        );
    }
}

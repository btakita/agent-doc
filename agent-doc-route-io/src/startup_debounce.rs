//! Route startup Lazily-current transition wait.

use agent_doc_crdt_relay_io::{CrdtReplicaEventReason, CurrentText};
use agent_doc_debounce::{
    CurrentAuthorityAdmission, SettleAction, SettleBudget, SettleDeferReason, SettleTimers,
};
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

/// Wait for Lazily's current-document authority before route dispatch.
///
/// Admission follows the same pure policy as preflight
/// (`agent_doc_debounce::current_authority_admission`, `#routestartupadmit`):
/// route waits for the authoritative text to EXIST, never for every editor
/// replica to acknowledge it. A `delivery_pending` observation already carries
/// the controller's canonical text — the operator's prompt included (the
/// JetBrains `Run Agent Doc` action writes its prompt marker before routing) —
/// and `delivery_converged` is an availability fact, not a receipt. Waiting on
/// it delayed or deferred dispatch (`route deferred ...: Lazily current
/// transition remained delivery_pending for 5000ms`) over a write the controller
/// already held. Route admits on that text immediately and fires one urgent
/// delivery drain without awaiting it (`#crdtpushdrain`).
///
/// States with no authoritative text (`missing_replica`, `current_pending`,
/// `authority_unavailable`) still wait and fail closed at the no-progress
/// budget. Disk mtime and filesystem typing markers are never authority.
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
    let budget = SettleBudget::from_no_progress(max_wait);

    loop {
        let current = observe(file, "route_startup_current_transition");
        let (ready, state) = match current {
            Ok(None | Some(CurrentText::Detached)) => (true, "detached"),
            Ok(Some(CurrentText::Current {
                live_editors,
                delivery_converged,
                ..
            })) => {
                match agent_doc_debounce::current_authority_admission(true, delivery_converged) {
                    CurrentAuthorityAdmission::Admit => (true, "lazily_current"),
                    CurrentAuthorityAdmission::AdmitWhileDeliveryPending => {
                        admit_while_delivery_pending(file, live_editors, start, &mut signal);
                        return Ok(());
                    }
                    CurrentAuthorityAdmission::WaitForAuthority => (false, "authority_unavailable"),
                }
            }
            Ok(Some(CurrentText::EditorAttachedMissingReplica)) => (false, "missing_replica"),
            Ok(Some(CurrentText::EditorSyncPending)) => (false, "current_pending"),
            Err(_) => (false, "authority_unavailable"),
        };

        // No authoritative text means no frontier to observe advancing: the
        // whole wait is one no-progress window.
        let timers = SettleTimers {
            stalled_for: start.elapsed(),
            total_elapsed: start.elapsed(),
            since_last_urgent_drain: None,
        };
        match agent_doc_debounce::settle_step(ready, false, timers, budget) {
            SettleAction::Ready => {
                eprintln!("[route] Lazily current transition settled ({state})");
                return Ok(());
            }
            SettleAction::Defer { reason } => {
                let detail = match reason {
                    SettleDeferReason::ProgressCeiling => format!(
                        "did not settle within {}ms (progress ceiling)",
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
            SettleAction::Wait { .. } => {}
        }

        std::thread::sleep(poll_interval);
    }
}

/// `#routestartupadmit`: admit route dispatch on the current authority while
/// replica delivery is pending. The urgent drain is fire-and-forget — its
/// outcome is logged, never awaited, and a failed request does not defer.
fn admit_while_delivery_pending<Signal>(
    file: &Path,
    live_editors: usize,
    start: Instant,
    signal: &mut Signal,
) where
    Signal: FnMut(&Path, CrdtReplicaEventReason, usize) -> Result<()>,
{
    let reason = CrdtReplicaEventReason::CanonicalProjection;
    let drain = match signal(file, reason, live_editors) {
        Ok(()) => "requested".to_string(),
        Err(error) => format!("failed:{error:#}").replace('\n', " "),
    };
    eprintln!(
        "[route] admitted on current authority while delivery is pending (live_editors={live_editors} urgent_drain={drain} reason={})",
        reason.token()
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_startup_admitted_on_current_authority file={} state=delivery_pending live_editors={} waited_ms={} urgent_drain={} (#routestartupadmit)",
            file.display(),
            live_editors,
            start.elapsed().as_millis(),
            drain,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn current(text: &str, live_editors: usize, delivery_converged: bool) -> CurrentText {
        CurrentText::Current {
            text: text.to_owned(),
            live_editors,
            delivery_converged,
            delivery_version: 1,
            semantics: None,
        }
    }

    #[test]
    fn route_dispatches_immediately_when_lazily_is_detached() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "settled prompt\n").unwrap();

        // A filesystem debounce would wait at least the whole idle window, so
        // make that window far larger than any scheduler stall: the assertion
        // then separates "debounced" from "dispatched now" under CPU load
        // instead of racing a 50ms wall clock (it flaked at 129ms under
        // parallel `make check` runs).
        let idle_window = Duration::from_secs(5);
        let start = Instant::now();
        await_idle_with_max_wait(&doc, idle_window, Duration::from_secs(10))
            .expect("detached Lazily authority authorizes immediate dispatch");
        assert!(
            start.elapsed() < idle_window / 2,
            "route must not impose a filesystem debounce when Lazily is detached (elapsed {:?})",
            start.elapsed()
        );
    }

    #[test]
    fn route_startup_admits_a_converged_authority_without_a_drain() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let signals = RefCell::new(Vec::new());

        await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            Duration::from_secs(5),
            |_file, _source| Ok(Some(current("prompt", 1, true))),
            |_file, reason, targets| {
                signals.borrow_mut().push((reason, targets));
                Ok(())
            },
        )
        .unwrap();
        assert!(signals.into_inner().is_empty());
    }

    /// `#routestartupadmit` regression for the recurring
    /// `route deferred ...: Lazily current transition remained delivery_pending
    /// for 5000ms`. The JetBrains `Run Agent Doc` action writes its prompt marker
    /// first, so route observed the controller already holding that text while a
    /// replica had not acknowledged it — and waited on the acknowledgement. A
    /// pending delivery carries the authoritative text: route must admit on the
    /// FIRST observation, with exactly one fire-and-forget urgent drain.
    #[test]
    fn route_startup_admits_a_pending_delivery_on_the_current_authority() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let observations = Cell::new(0usize);
        let signals = RefCell::new(Vec::new());

        // A long budget: the old behaviour waited (and re-polled) here, then
        // deferred once it expired.
        let outcome = await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            Duration::from_secs(5),
            |_file, _source| {
                observations.set(observations.get() + 1);
                Ok(Some(current("operator prompt in flight", 2, false)))
            },
            |_file, reason, targets| {
                signals.borrow_mut().push((reason, targets));
                Ok(())
            },
        );

        assert!(
            outcome.is_ok(),
            "a pending delivery must not defer route: {outcome:?}"
        );
        assert_eq!(
            observations.get(),
            1,
            "route must admit on the first observation, not poll for delivery convergence"
        );
        assert_eq!(
            signals.into_inner(),
            vec![(CrdtReplicaEventReason::CanonicalProjection, 2)],
            "route still asks the relay once to push the delivery on"
        );
    }

    /// The urgent drain is fire-and-forget: a failed request is logged, never a
    /// reason to wait or defer.
    #[test]
    fn route_startup_admits_when_the_urgent_drain_request_fails() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let signals = Cell::new(0usize);

        let outcome = await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            Duration::from_secs(5),
            |_file, _source| Ok(Some(current("prompt", 1, false))),
            |_file, _reason, _targets| {
                signals.set(signals.get() + 1);
                anyhow::bail!("relay socket unavailable")
            },
        );

        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(signals.get(), 1);
    }

    /// States with no authoritative text still wait and fail closed, and never
    /// request a drain (there is no delivery to push).
    #[test]
    fn route_startup_still_fails_closed_without_authoritative_text() {
        let cases: [(&str, fn() -> Result<Option<CurrentText>>); 3] = [
            ("missing_replica", || {
                Ok(Some(CurrentText::EditorAttachedMissingReplica))
            }),
            ("current_pending", || {
                Ok(Some(CurrentText::EditorSyncPending))
            }),
            ("authority_unavailable", || {
                anyhow::bail!("controller unreachable")
            }),
        ];
        for (state, observation) in cases {
            let dir = tempfile::TempDir::new().unwrap();
            let doc = dir.path().join("session.md");
            let observations = Cell::new(0usize);
            let signals = Cell::new(0usize);

            let outcome = await_idle_with_max_wait_and_effects(
                &doc,
                Duration::from_millis(10),
                Duration::from_millis(300),
                |_file, _source| {
                    observations.set(observations.get() + 1);
                    observation()
                },
                |_file, _reason, _targets| {
                    signals.set(signals.get() + 1);
                    Ok(())
                },
            );

            let message = format!("{:#}", outcome.expect_err(state));
            assert!(
                message.contains(&format!("remained {state}")),
                "{state}: {message}"
            );
            assert!(
                observations.get() > 1,
                "{state}: route must keep waiting for authority before failing closed"
            );
            assert_eq!(signals.get(), 0, "{state}: nothing to drain");
        }
    }

    /// A missing authority that appears within the budget is admitted.
    #[test]
    fn route_startup_admits_once_authority_appears() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let observations = Cell::new(0usize);

        await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            Duration::from_secs(5),
            |_file, _source| {
                let n = observations.get();
                observations.set(n + 1);
                if n < 2 {
                    Ok(Some(CurrentText::EditorSyncPending))
                } else {
                    Ok(Some(current("prompt", 1, false)))
                }
            },
            |_file, _reason, _targets| Ok(()),
        )
        .unwrap();
        assert_eq!(observations.get(), 3);
    }

    /// `#routestartupadmit` SimWorld: the operator types a prompt into the
    /// exchange and fires `Run Agent Doc`. The relay hub already holds the edit
    /// in its canonical text, but a second live replica has not acknowledged it.
    /// Route must dispatch on that canonical text at once — the urgent drain is
    /// requested but not awaited — and delivery still converges afterwards.
    #[test]
    fn simworld_operator_prompt_in_flight_is_admitted_by_route_startup() {
        use agent_doc_document_realtime::crdt_relay::RelayHub;
        use agent_doc_merge::crdt_sync::ReplicaState;

        fn drain(hub: &mut RelayHub, client: u64) {
            for update in hub.pending_updates(client).unwrap() {
                hub.ack_delivery(client, &update.patch_id, update.generation)
                    .unwrap();
            }
        }

        let committed = "# Session\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let mut hub = RelayHub::new(1);
        hub.register(2).unwrap(); // the operator's editor
        hub.register(3).unwrap(); // a second live replica
        hub.apply_canonical_replace("", committed).unwrap();
        drain(&mut hub, 2);
        drain(&mut hub, 3);
        assert!(hub.delivery_converged());

        // The operator's prompt reaches the canonical authority from replica 2.
        let editor = ReplicaState::from_encoded(2, &hub.canonical_encoded_state()).unwrap();
        let anchor = "<!-- agent:exchange -->\n";
        let offset = editor.text().find(anchor).unwrap() + anchor.len();
        editor.apply_local_edit(offset as u32, 0, "please fix the route wait\n");
        let update = editor.diff(&ReplicaState::new(99).state_vector()).unwrap();
        hub.relay_update(2, &update).unwrap();
        assert!(
            !hub.delivery_converged(),
            "the fixture must hold delivery pending"
        );

        let hub = RefCell::new(hub);
        let admitted = RefCell::new(None::<String>);
        let drains = RefCell::new(Vec::new());
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("session.md");

        await_idle_with_max_wait_and_effects(
            &doc,
            Duration::from_millis(10),
            Duration::from_secs(5),
            |_file, _source| {
                let hub = hub.borrow();
                let text = hub.canonical_text();
                *admitted.borrow_mut() = Some(text.clone());
                Ok(Some(CurrentText::Current {
                    text,
                    live_editors: hub.live_count(),
                    delivery_converged: hub.delivery_converged(),
                    delivery_version: 1,
                    semantics: None,
                }))
            },
            // Fire-and-forget: record the request, deliver nothing yet.
            |_file, reason, targets| {
                drains.borrow_mut().push((reason, targets));
                Ok(())
            },
        )
        .expect("route must admit an operator prompt the controller already holds");

        let admitted = admitted.into_inner().unwrap();
        assert!(admitted.contains("please fix the route wait\n"));
        let mut hub = hub.into_inner();
        assert!(
            !hub.delivery_converged(),
            "route admitted while delivery was still pending"
        );
        assert_eq!(drains.into_inner().len(), 1);

        // The relay delivers the edit afterwards; the admitted text was final.
        drain(&mut hub, 3);
        assert!(hub.delivery_converged());
        assert_eq!(hub.canonical_text(), admitted);
    }
}

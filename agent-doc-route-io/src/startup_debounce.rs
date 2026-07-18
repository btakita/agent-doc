//! Route startup Lazily-current transition wait.

use agent_doc_crdt_relay_io::{CrdtReplicaEventReason, CurrentText};
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

/// `#crdtpushdrain`: how often route re-requests an urgent CRDT delivery drain
/// while the current transition stays `delivery_pending`. Bounded well under the
/// route `max_wait` budget so a drain that applies nothing gets several attempts
/// instead of one, while staying far coarser than the 100ms observe poll.
const URGENT_DRAIN_RETRY_INTERVAL: Duration = Duration::from_millis(750);

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
    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();
    let _ = debounce;
    let mut last_urgent_drain: Option<Instant> = None;

    loop {
        let current = observe(file, "route_startup_current_transition");
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
        if ready {
            eprintln!("[route] Lazily current transition settled ({state})");
            return Ok(());
        }

        // #crdtpushdrain: re-request on a bounded cadence rather than once. A single
        // urgent drain can legitimately apply nothing — `drainRemoteUpdatesFor`
        // returns early while the path has pending local edits or is mid editor
        // apply, and the forwarder may not be registered yet. Its only follow-up is
        // the *gated* `requestRemoteDrain`, which an idle document's escalated no-op
        // backoff suppresses, so a one-shot latch left the remaining budget polling a
        // frontier nobody would pull and route deferred at `max_wait`.
        let urgent_drain_due = last_urgent_drain
            .is_none_or(|last| last.elapsed() >= URGENT_DRAIN_RETRY_INTERVAL);
        if let Some(targets) = drain_targets.filter(|_| urgent_drain_due) {
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

        if start.elapsed() >= max_wait {
            anyhow::bail!(
                "route deferred for {}: Lazily current transition remained {} for {}ms; retry after it settles",
                file.display(),
                state,
                max_wait.as_millis()
            );
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
            signals
                .iter()
                .all(|(reason, targets)| *reason
                    == CrdtReplicaEventReason::AckRecoveryForceRefresh
                    && *targets == 2),
            "every retry must carry the same force-refresh reason and live target count"
        );
    }
}

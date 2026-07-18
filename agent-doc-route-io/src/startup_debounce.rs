//! Route startup Lazily-current transition wait.

use agent_doc_crdt_relay_io::{CrdtReplicaEventReason, CurrentText};
use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

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
    let mut urgent_drain_requested = false;

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

        if let Some(targets) = drain_targets.filter(|_| !urgent_drain_requested) {
            urgent_drain_requested = true;
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
}

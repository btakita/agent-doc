//! Route startup Lazily-current transition wait.

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
    let poll_interval = Duration::from_millis(100);
    let start = Instant::now();
    let _ = debounce;

    loop {
        use agent_doc_crdt_relay_io::CurrentText;
        let current =
            agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
                file,
                "route_startup_current_transition",
            );
        let (ready, state) = match current {
            Ok(None | Some(CurrentText::Detached)) => (true, "detached"),
            Ok(Some(CurrentText::Current {
                delivery_converged: true,
                ..
            })) => (true, "lazily_current"),
            Ok(Some(CurrentText::Current { .. })) => (false, "delivery_pending"),
            Ok(Some(CurrentText::EditorAttachedMissingReplica)) => (false, "missing_replica"),
            Ok(Some(CurrentText::EditorSyncPending)) => (false, "current_pending"),
            Err(_) => (false, "authority_unavailable"),
        };
        if ready {
            eprintln!("[route] Lazily current transition settled ({state})");
            return Ok(());
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
}

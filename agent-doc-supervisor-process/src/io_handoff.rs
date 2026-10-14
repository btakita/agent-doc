//! Supervisor PTY I/O handoff barrier for child-preserving `execve`.
//!
//! The stale supervisor is allowed to replace its own process image while the
//! harness child keeps running.  A process-image swap must not happen after an
//! I/O worker consumed bytes into userspace but before it forwarded them.  All
//! PTY input/output transactions therefore cross this gate; the reexec effect
//! freezes it before handing the PTY fd to the replacement image.

use parking_lot::{Mutex, MutexGuard};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub struct IoHandoffGate {
    transaction: Mutex<()>,
    output_filter_at_boundary: AtomicBool,
}

impl Default for IoHandoffGate {
    fn default() -> Self {
        Self {
            transaction: Mutex::new(()),
            output_filter_at_boundary: AtomicBool::new(true),
        }
    }
}

impl IoHandoffGate {
    /// Enter one complete byte-transfer transaction.
    pub fn forwarding(&self) -> MutexGuard<'_, ()> {
        self.transaction.lock()
    }

    /// Freeze new byte consumption until the caller either execs or drops the
    /// guard after a refused/failed handoff.
    pub fn freeze(&self) -> MutexGuard<'_, ()> {
        self.transaction.lock()
    }

    /// Record whether the stateful terminal filter has an incomplete escape
    /// sequence buffered in the outgoing image.
    pub fn set_output_filter_at_boundary(&self, at_boundary: bool) {
        self.output_filter_at_boundary
            .store(at_boundary, Ordering::Release);
    }

    pub fn output_filter_at_boundary(&self) -> bool {
        self.output_filter_at_boundary.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn freeze_waits_for_the_complete_forwarding_transaction() {
        let gate = Arc::new(IoHandoffGate::default());
        let forwarding = gate.forwarding();
        let (tx, rx) = mpsc::channel();
        let contender = gate.clone();
        let handle = std::thread::spawn(move || {
            let _freeze = contender.freeze();
            tx.send(()).unwrap();
        });

        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(forwarding);
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn output_filter_boundary_is_part_of_handoff_evidence() {
        let gate = IoHandoffGate::default();
        assert!(gate.output_filter_at_boundary());
        gate.set_output_filter_at_boundary(false);
        assert!(!gate.output_filter_at_boundary());
        gate.set_output_filter_at_boundary(true);
        assert!(gate.output_filter_at_boundary());
    }
}

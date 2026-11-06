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
    #[cfg(target_os = "linux")]
    use std::time::Instant;

    /// `#wallclockassertsweep2`: a wait that fails only when the awaited event
    /// never happens. Not a latency bound.
    const HANG_CEILING: Duration = Duration::from_secs(30);

    #[cfg(target_os = "linux")]
    fn parked_in_futex(tid: u64) -> bool {
        let text = std::fs::read_to_string(format!("/proc/{tid}/syscall")).unwrap_or_default();
        text.split_whitespace()
            .next()
            .and_then(|number| number.parse::<i64>().ok())
            == Some(libc::SYS_futex)
    }

    #[cfg(target_os = "linux")]
    fn assert_freeze_still_held(done: &mpsc::Receiver<()>) {
        match done.try_recv() {
            Ok(()) => panic!("freeze returned while the forwarding transaction was still held"),
            Err(mpsc::TryRecvError::Disconnected) => {
                panic!("contender ended before freeze returned");
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    #[test]
    fn freeze_waits_for_the_complete_forwarding_transaction() {
        let gate = Arc::new(IoHandoffGate::default());
        let forwarding = gate.forwarding();
        let (tid_tx, tid_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let contender = Arc::clone(&gate);
        let handle = std::thread::spawn(move || {
            // Published before `freeze`, so the waiter below cannot mistake
            // this send for the mutex park.
            #[cfg(target_os = "linux")]
            {
                let tid = unsafe { libc::syscall(libc::SYS_gettid) };
                assert!(tid > 0, "gettid failed");
                tid_tx.send(tid as u64).unwrap();
            }
            #[cfg(not(target_os = "linux"))]
            tid_tx.send(0u64).unwrap();
            let _freeze = contender.freeze();
            done_tx.send(()).unwrap();
        });

        let tid = tid_rx
            .recv_timeout(HANG_CEILING)
            .expect("contender never started");

        // Linux observes the contender parked in `SYS_futex` (parking_lot's
        // parker) while this thread still holds the forwarding guard. A
        // `freeze` that did not wait would have sent already. Other targets
        // have no `/proc/<tid>/syscall` equivalent; CI runs this on Linux.
        #[cfg(target_os = "linux")]
        {
            let deadline = Instant::now() + HANG_CEILING;
            loop {
                assert_freeze_still_held(&done_rx);
                if parked_in_futex(tid) {
                    std::thread::yield_now();
                    assert_freeze_still_held(&done_rx);
                    if parked_in_futex(tid) {
                        break;
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "contender never parked in freeze while the forwarding transaction was held"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = tid;

        drop(forwarding);
        done_rx
            .recv_timeout(HANG_CEILING)
            .expect("freeze did not resume after the forwarding transaction dropped");
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

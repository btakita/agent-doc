//! With the controller's allocator policy applied, a
//! burst of large transient buffers on many connection-like threads does not
//! leave the process resident at its high-water mark.
//!
//! This is its own test binary (one process) because the policy is
//! process-wide and the assertion reads this process's resident set.

use agent_doc_controller_io::heap_hygiene::{
    configure_process_allocator, resident_bytes, trim_heap,
};

const THREADS: usize = 24;
const ROUNDS: usize = 6;
const MIB: usize = 1 << 20;

/// Roughly what one replica RPC on a 500 KB session document allocates: a few
/// multi-megabyte op/encode buffers plus a spray of small per-op allocations.
fn replica_rpc_shaped_burst(seed: usize) -> usize {
    let mut touched = 0usize;
    let mut large = Vec::new();
    for step in 0..4 {
        let size = (2 + (seed + step * 7) % 30) * MIB;
        let mut buffer = vec![0u8; size];
        for page in buffer.iter_mut().step_by(4096) {
            *page = 1;
        }
        touched += buffer.len();
        large.push(buffer);
    }
    let small: Vec<Vec<u8>> = (0..20_000)
        .map(|index| vec![u8::try_from(index % 251).unwrap_or(0); 64 + index % 512])
        .collect();
    touched += small.iter().map(Vec::len).sum::<usize>();
    drop(small);
    drop(large);
    touched
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn controller_heap_returns_transient_crdt_buffers_after_a_burst() {
    let policy = configure_process_allocator();
    assert!(policy.supported && policy.applied, "{policy:?}");
    trim_heap();
    let baseline = resident_bytes().expect("read /proc/self/statm");

    let mut touched = 0usize;
    for round in 0..ROUNDS {
        let handles = (0..THREADS)
            .map(|thread| {
                std::thread::Builder::new()
                    .name(format!("burst-{round}-{thread}"))
                    .spawn(move || replica_rpc_shaped_burst(round * THREADS + thread))
                    .expect("spawn burst thread")
            })
            .collect::<Vec<_>>();
        for handle in handles {
            touched += handle.join().expect("burst thread");
        }
    }
    trim_heap();
    let after = resident_bytes().expect("read /proc/self/statm");
    let retained = after.saturating_sub(baseline);
    eprintln!(
        "touched={} MiB baseline={} MiB after={} MiB retained={} MiB",
        touched / MIB,
        baseline / (MIB as u64),
        after / (MIB as u64),
        retained / (MIB as u64),
    );
    assert!(
        touched > 4 * 1024 * MIB,
        "the burst must be large enough to matter: {touched}"
    );
    // Freed memory goes back: at most a few arenas' worth of small-allocation
    // slack survives, never the burst's multi-gigabyte high-water mark.
    assert!(
        retained < 96 * MIB as u64,
        "controller heap kept {} MiB resident after a {} MiB transient burst",
        retained / (MIB as u64),
        touched / MIB
    );
}

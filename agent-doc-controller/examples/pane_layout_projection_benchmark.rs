//! Run with `cargo run --release -p agent-doc-controller --example pane_layout_projection_benchmark`.

use agent_doc_controller::pane_layout::LatestProjectionWorkerState;
use std::hint::black_box;
use std::time::Instant;

const BATCHES: u64 = 20_000;
const WRITES_PER_BATCH: u64 = 256;

fn main() {
    let collapse_started = Instant::now();
    for batch in 0..BATCHES {
        let mut state = LatestProjectionWorkerState::default();
        let first_generation = batch * WRITES_PER_BATCH + 1;
        assert!(black_box(&mut state).schedule(black_box(first_generation)));
        for offset in 1..WRITES_PER_BATCH {
            assert!(!black_box(&mut state).schedule(black_box(first_generation + offset)));
        }
        assert_eq!(
            black_box(state.pending_generation()),
            first_generation + WRITES_PER_BATCH - 1
        );
    }
    let collapse_elapsed = collapse_started.elapsed();

    let cancellation_started = Instant::now();
    for batch in 0..(BATCHES * WRITES_PER_BATCH) {
        let mut state = LatestProjectionWorkerState::default();
        assert!(black_box(&mut state).schedule(black_box(batch * 2 + 1)));
        assert!(!black_box(&mut state).schedule(black_box(batch * 2 + 2)));
        assert!(black_box(&state).is_superseded(black_box(batch * 2 + 1)));
    }
    let cancellation_elapsed = cancellation_started.elapsed();

    println!(
        "latest_projection_collapse: {:.2} ns/write ({} writes)",
        collapse_elapsed.as_nanos() as f64 / (BATCHES * WRITES_PER_BATCH) as f64,
        BATCHES * WRITES_PER_BATCH
    );
    println!(
        "stale_projection_cancel_check: {:.2} ns/check ({} checks)",
        cancellation_elapsed.as_nanos() as f64 / (BATCHES * WRITES_PER_BATCH) as f64,
        BATCHES * WRITES_PER_BATCH
    );
}

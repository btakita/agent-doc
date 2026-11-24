//! `#q-8383c546c6ca` measurement of the reads behind preflight's `repair` and
//! `stuck_cycle_detection` phases (`--release --ignored --nocapture`), against
//! an existing project document `AGENT_DOC_REPAIR_BENCH_FILE` names: a
//! private, re-keyed copy of a live project whose state database and
//! `ops.log` carry the live ledgers, so each read costs what it costs live.
//! Without the variable nothing is measured.

use std::time::Instant;

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = f();
    eprintln!("{label:>40}: {:>10.2?}", start.elapsed());
    value
}

#[test]
#[ignore]
fn bench_repair_phase_reads() {
    let Ok(file) = std::env::var("AGENT_DOC_REPAIR_BENCH_FILE") else {
        eprintln!("AGENT_DOC_REPAIR_BENCH_FILE unset; nothing measured");
        return;
    };
    let file = std::path::PathBuf::from(file);
    let rounds: usize = std::env::var("AGENT_DOC_REPAIR_BENCH_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    for round in 0..rounds {
        eprintln!("--- round {round}");
        time("repair: last ops event (commit missing)", || {
            agent_doc_ops_log_io::detect_write_completed_commit_missing(&file).unwrap()
        });
        time("repair: closeout projection", || {
            agent_doc_cycle_state_io::load_with_closeout_projection(&file).unwrap()
        });
        time("repair: pending document write", || {
            agent_doc_document_realtime_io::pending_document_write(&file)
        });
        time("stuck: compacted capture reconcile", || {
            agent_doc_flow_io::closeout::reconcile_compacted_committed_capture(&file).unwrap()
        });
        time("stuck: stuck captured cycle", || {
            agent_doc_flow_io::closeout::stuck_captured_cycle(&file)
        });
        time("stuck: single-read settle+detect sweep", || {
            agent_doc_flow_io::closeout::settle_compacted_capture_and_detect_stuck(&file, true)
        });
        time("stuck: active capture", || {
            agent_doc_capture_io::load_active(&file).unwrap()
        });
    }
}

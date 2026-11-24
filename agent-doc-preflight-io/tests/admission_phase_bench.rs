//! `#incrementalreconcile` measurement of the read-only queue projections the
//! preflight admission runs and of the commit-time continuation marker
//! (`--release --ignored --nocapture`), on a private copy of the document
//! `AGENT_DOC_ADMISSION_BENCH_DOC` names, in a temporary project
//! (`AGENT_DOC_ADMISSION_BENCH_ROUNDS` rounds, default 2). Without the
//! variable nothing is measured.

use std::time::Instant;

fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = f();
    eprintln!("{label:>34}: {:>10.2?}", start.elapsed());
    value
}

#[test]
#[ignore]
fn bench_admission_phases() {
    let Ok(source) = std::env::var("AGENT_DOC_ADMISSION_BENCH_DOC") else {
        eprintln!("AGENT_DOC_ADMISSION_BENCH_DOC unset; nothing measured");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.path().join("tasks")).unwrap();
    let file = root.path().join("tasks/session.md");
    let content = std::fs::read_to_string(source).unwrap();
    std::fs::write(&file, &content).unwrap();
    let rounds: usize = std::env::var("AGENT_DOC_ADMISSION_BENCH_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    for round in 0..rounds {
        eprintln!("--- round {round}");
        let state = time("queue_admission_inspect", || {
            agent_doc_preflight_io::inspect_queue_state_from_content_with_skipped(
                &file,
                None,
                content.clone(),
                &Default::default(),
            )
            .unwrap()
        });
        let admitted = state
            .authoritative_content
            .clone()
            .unwrap_or_else(|| content.clone());
        time("queue_admission_payload", || {
            let _ = agent_doc_queue::prompt_identity::project(
                &admitted,
                &agent_doc_hash::document_id_for_path(&file),
            );
            agent_doc_queue_io::sync_payload::build_project_queue_sync_payload(
                &file,
                &admitted,
                Some(&content),
            )
            .unwrap()
        });
        let pending = time("tier_capture_scope", || {
            let pending = agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_or_warn(
                &file, &admitted,
            );
            let _ = agent_doc_queue_io::queue_claim::claimed_items_for_content(&file, &admitted)
                .with_heads(&pending);
            pending
        });
        time("owner_queue_dispatch_projection", || {
            let _ =
                agent_doc_preflight_io::queue_subagent_dispatch_entries(&file, &admitted, &pending);
            let _ = agent_doc_queue_io::subagent_pool::pool_refill_for_content(
                &file,
                &admitted,
                agent_doc_queue::subagent_pool::PoolRefillTrigger::Plan,
            );
        });
        time("reconcile_marker", || {
            agent_doc_queue_io::queue_continuation::reconcile_marker(&file, "bench")
        });
    }
}

/// `#q-c71d106debc4` measurement of the closeout-time continuation reconcile against
/// an existing project document `AGENT_DOC_RECONCILE_BENCH_FILE` names (a
/// private, re-keyed copy whose state database carries the live claim, receipt
/// and lineage ledgers). It times the inline reconcile the closeout used to
/// run, then what the closeout pays now (publish the pending fact and notify
/// the controller; the notify is stubbed) and the background run that
/// consumes it. Without the variable nothing is measured. It mutates that
/// copy's ledgers, so restore it between runs.
#[test]
#[ignore]
fn bench_reconcile_marker_phases() {
    let Ok(file) = std::env::var("AGENT_DOC_RECONCILE_BENCH_FILE") else {
        eprintln!("AGENT_DOC_RECONCILE_BENCH_FILE unset; nothing measured");
        return;
    };
    let file = std::path::PathBuf::from(file);
    let rounds: usize = std::env::var("AGENT_DOC_ADMISSION_BENCH_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    use agent_doc_queue_io::continuation_reconcile as reconcile;
    for round in 0..rounds {
        eprintln!("--- round {round}");
        time("inline reconcile_marker", || {
            agent_doc_queue_io::queue_continuation::reconcile_marker(&file, "bench")
        });
        let deferred = time("closeout publish+notify", || {
            reconcile::reconcile_after_closeout_with(&file, "bench", |_| Ok("scheduled".into()))
        });
        assert!(matches!(
            deferred,
            reconcile::CloseoutReconcile::Deferred { .. }
        ));
        time("background run_pending", || {
            reconcile::run_pending(&file, "bench").unwrap()
        });
        time("stop settle (nothing pending)", || {
            reconcile::settle_pending(&file, reconcile::STOP_SETTLE_WAIT, "bench")
        });
    }
}

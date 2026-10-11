//! `#binaryownedcli`: binary-owned CLI writes against a REAL editor replica
//! whose delivery projection converges only after the post-write barrier
//! refused.
//!
//! Observed 2026-10-11 on agent-doc-bugs.md: `backlog add`, `backlog remove
//! --contains`, `queue sync`, and `queue consume --id` each exited non-zero
//! with `retained=delivery_projection_pending` plus a dogfood
//! `projection_failure` prompt, although the write converged seconds later.
//! Each test drives the real command function through the real runtime
//! effects, embedded relay, and a registered replica; the simulated editor
//! acknowledges only once the command has entered its convergence await (an
//! event edge, never a wall-clock sleep).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agent_doc_write_converge_io::retained_cli_write::{
    RETAINED_CLI_WRITE_DEFERRED_TOKEN, retained_cli_write_deferral, with_binary_owned_cli_write,
};

struct TestQueueCommandEffects;

impl agent_doc_queue_io::queue_cmd::QueueCommandEffects for TestQueueCommandEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        agent_doc_write_converge_io::converge_document_or_disk(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            target_content,
            source_content,
            reason,
        )
    }

    fn consume_free_text_queue_prompts_force_disk(
        &self,
        _file: &Path,
        _count: usize,
    ) -> anyhow::Result<Option<agent_doc_queue_io::queue_cmd::QueueCommandConsumeOutcome>> {
        unreachable!("not exercised")
    }

    fn consume_free_text_queue_prompts_with_outcome(
        &self,
        _file: &Path,
        _count: usize,
    ) -> anyhow::Result<Option<agent_doc_queue_io::queue_cmd::QueueCommandConsumeOutcome>> {
        unreachable!("not exercised")
    }

    fn strike_orphan_id_backed_queue_head(&self, file: &Path, id: &str) -> anyhow::Result<bool> {
        agent_doc_queue_io::queue_consume::strike_orphan_id_backed_queue_head(
            file,
            id,
            &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
        )
    }

    fn acknowledge_open_id_backed_queue_head(
        &self,
        _file: &Path,
        _id: &str,
    ) -> anyhow::Result<bool> {
        unreachable!("not exercised")
    }

    fn acknowledge_paused_free_text_queue_head(
        &self,
        _file: &Path,
        _expected_head: &str,
    ) -> anyhow::Result<Option<agent_doc_queue_io::queue_cmd::QueueCommandConsumeOutcome>> {
        unreachable!("not exercised")
    }

    fn prune_noise_queue_heads(&self, _file: &Path) -> anyhow::Result<usize> {
        unreachable!("not exercised")
    }
}

fn seed_reliable_sync_open(file: &Path, tag: &str) {
    let document_hash = agent_doc_hash::document_id_for_path(file);
    agent_doc_reliable_sync_io::global_liveness_plane()
        .lock()
        .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
            document_hash,
            pid: std::process::id().into(),
            tag: tag.to_string(),
        }]);
}

/// A document held by a live editor replica that has not acknowledged
/// anything yet.
fn attached_document(content: &str, identity: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
    let file = dir.path().join("session.md");
    std::fs::write(&file, content).unwrap();
    agent_doc_crdt_relay_io::register_embedded_relay_route_for_file(&file).unwrap();
    let file = file.canonicalize().unwrap();
    seed_reliable_sync_open(&file, identity);
    agent_doc_crdt_relay_io::register_replica_for_file(&file, identity)
        .unwrap()
        .expect("editor replica should attach");
    (dir, file)
}

/// Project every queued delivery into the simulated editor and acknowledge it.
fn project_pending_deliveries(file: &Path, identity: &str) {
    let pull = agent_doc_crdt_relay_io::pull_replica_updates_for_file(file, identity)
        .expect("pull CRDT delivery")
        .expect("test editor remains attached");
    if let Some(update) = pull.updates.last() {
        assert_eq!(
            agent_doc_crdt_relay_io::observe_replica_projection_for_file(
                file,
                identity,
                &update.expected_content_hash,
            )
            .expect("project CRDT delivery"),
            Some(true),
        );
    }
}

/// Run `command` as a binary-owned CLI write while a simulated editor
/// acknowledges each delivery only AFTER the command entered its retained
/// convergence await. Returns the command result and how many awaits ran.
fn run_against_late_converging_editor(
    file: &Path,
    identity: &'static str,
    wait: Duration,
    command: impl FnOnce(&Path) -> anyhow::Result<()> + Send + 'static,
) -> (anyhow::Result<()>, usize) {
    let done = Arc::new(AtomicBool::new(false));
    let cli_file = file.to_path_buf();
    let cli_done = done.clone();
    let cli = std::thread::spawn(move || {
        let result = agent_doc_element_backlog_io::with_backlog_command_effects(
            &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
            || with_binary_owned_cli_write(wait, || command(&cli_file)),
        );
        cli_done.store(true, Ordering::SeqCst);
        agent_doc_document_realtime_io::test_support_wake_retained_cli_write_waiters();
        result
    });
    let mut seen = 0usize;
    while let Some(entered) =
        agent_doc_document_realtime_io::test_support_await_retained_cli_write_await_entered(
            file, seen, &done,
        )
    {
        seen = entered;
        project_pending_deliveries(file, identity);
    }
    (cli.join().expect("cli thread"), seen)
}

fn authority_text(file: &Path) -> String {
    match agent_doc_crdt_relay_io::current_text_for_file(file).unwrap() {
        agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
        other => panic!("expected attached authority text, got {other:?}"),
    }
}

fn ops_log(file: &Path) -> String {
    std::fs::read_to_string(file.parent().unwrap().join(".agent-doc/logs/ops.log"))
        .unwrap_or_default()
}

fn assert_retained_then_converged(file: &Path, awaits: usize, result: anyhow::Result<()>) {
    if let Err(err) = &result {
        panic!("a retained write that converges must succeed: {err:#}");
    }
    assert!(
        awaits >= 1,
        "test setup: the barrier must have retained the write before the editor converged"
    );
    let log = ops_log(file);
    assert!(
        log.contains("visible_write_delivery_projection_deferred"),
        "the write was retained first:\n{log}"
    );
    assert!(
        log.contains("binary_owned_cli_write_retained_settled")
            && log.contains("outcome=converged"),
        "the retained write settled through the convergence edge:\n{log}"
    );
    assert!(!log.contains("outcome=deferred"), "{log}");
}

const BACKLOG_DOC: &str = "---\nagent_doc_session: binaryownedcli\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n- [ ] [#keepme] Keep this open item\n- [ ] [#dropme] Remove this stale item\n<!-- /agent:backlog -->\n";

#[test]
fn backlog_add_converges_after_retention_and_succeeds() {
    let identity = "binaryownedcli-backlog-add";
    let (_dir, file) = attached_document(BACKLOG_DOC, identity);
    let (result, awaits) =
        run_against_late_converging_editor(&file, identity, Duration::from_secs(30), |file| {
            // `add_verified` re-reads the document after the write
            // (`#addreadback`); it only passes if convergence was awaited.
            agent_doc_element_backlog_io::backlog_cmd::add_verified(
                file,
                "Track the late converging write",
                false,
            )
        });
    assert_retained_then_converged(&file, awaits, result);
    let text = authority_text(&file);
    assert!(text.contains("Track the late converging write"), "{text}");
    assert!(text.contains("[#keepme]"), "{text}");
}

#[test]
fn backlog_remove_contains_converges_after_retention_and_succeeds() {
    let identity = "binaryownedcli-backlog-remove";
    let (_dir, file) = attached_document(BACKLOG_DOC, identity);
    let (result, awaits) =
        run_against_late_converging_editor(&file, identity, Duration::from_secs(30), |file| {
            agent_doc_element_backlog_io::backlog_cmd::remove(file, "stale item", true)
        });
    assert_retained_then_converged(&file, awaits, result);
    let text = authority_text(&file);
    assert!(!text.contains("[#dropme]"), "{text}");
    assert!(text.contains("[#keepme]"), "{text}");
}

#[test]
fn queue_sync_converges_after_retention_and_succeeds() {
    let identity = "binaryownedcli-queue-sync";
    let content = concat!(
        "---\nagent_doc_session: binaryownedcli\nagent_doc_format: template\nqueue_active: false\n---\n\n",
        "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#alpha] :inbox_tray: add me\n",
        "<!-- /agent:backlog -->\n",
    );
    let (_dir, file) = attached_document(content, identity);
    let (result, awaits) =
        run_against_late_converging_editor(&file, identity, Duration::from_secs(30), |file| {
            agent_doc_queue_io::queue_cmd::sync_with_effects(&TestQueueCommandEffects, file)
        });
    assert_retained_then_converged(&file, awaits, result);
    let text = authority_text(&file);
    let queue =
        &text[text.find("<!-- agent:queue").unwrap()..text.find("<!-- /agent:queue").unwrap()];
    assert!(queue.contains("[#alpha]"), "{text}");
    // A retained `queue sync` used to exit before its own snapshot checkpoint,
    // so the next preflight diffed the binary's write as operator input
    // (`[user+]`). Settling at the write boundary lets that checkpoint run.
    let baseline = agent_doc_snapshot_io::load_document_baseline(&file)
        .unwrap()
        .expect("queue sync checkpoints the baseline after a converged write");
    let baseline_queue = &baseline
        [baseline.find("<!-- agent:queue").unwrap()..baseline.find("<!-- /agent:queue").unwrap()];
    assert!(baseline_queue.contains("[#alpha]"), "{baseline}");
}

const ORPHAN_DOC: &str = concat!(
    "---\nagent_doc_session: binaryownedcli\nagent_doc_format: template\nqueue_active: true\n---\n\n",
    "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
    "<!-- agent:queue auto -->\n",
    "- do [#orphangone]\n",
    "- do [#liveone]\n",
    "<!-- /agent:queue -->\n\n",
    "<!-- agent:backlog -->\n",
    "- [ ] [#liveone] still open and drainable\n",
    "<!-- /agent:backlog -->\n",
);

#[test]
fn queue_consume_id_converges_after_retention_and_succeeds() {
    let identity = "binaryownedcli-queue-consume-id";
    let (_dir, file) = attached_document(ORPHAN_DOC, identity);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &file,
        ORPHAN_DOC,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    let (result, awaits) =
        run_against_late_converging_editor(&file, identity, Duration::from_secs(30), |file| {
            agent_doc_queue_io::queue_cmd::consume_orphan_id(
                &TestQueueCommandEffects,
                file,
                "orphangone",
            )
        });
    assert_retained_then_converged(&file, awaits, result);
    let text = authority_text(&file);
    assert!(text.contains("~~do [#orphangone]~~"), "{text}");
    assert!(text.contains("- do [#liveone]\n"), "{text}");
    // The strike's snapshot sync runs after the converged write, so the next
    // preflight does not read the binary's strike as operator input.
    let baseline = agent_doc_snapshot_io::load_document_baseline(&file)
        .unwrap()
        .expect("baseline");
    assert!(baseline.contains("~~do [#orphangone]~~"), "{baseline}");
}

/// The bound expired while the write was still retained: a typed deferral
/// (exit 75 in `main`, no dogfood prompt) that keeps the write durable in the
/// authority and touches no disk.
#[test]
fn a_still_retained_write_after_the_bound_is_a_deferral_not_a_failure() {
    let identity = "binaryownedcli-deferred";
    let (_dir, file) = attached_document(BACKLOG_DOC, identity);
    let cli_file = file.clone();
    let result = std::thread::spawn(move || {
        agent_doc_element_backlog_io::with_backlog_command_effects(
            &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
            || {
                with_binary_owned_cli_write(Duration::ZERO, || {
                    agent_doc_element_backlog_io::backlog_cmd::remove(&cli_file, "stale item", true)
                })
            },
        )
    })
    .join()
    .unwrap();
    let err = result.expect_err("an unacknowledged write cannot converge in a zero bound");
    let deferral = retained_cli_write_deferral(&err).unwrap_or_else(|| {
        panic!("the expired bound must be the typed deferral, not a failure: {err:#}")
    });
    assert_eq!(deferral.source, "pending_write");
    let rendered = format!("{err:#}");
    assert!(
        rendered.contains(RETAINED_CLI_WRITE_DEFERRED_TOKEN),
        "{rendered}"
    );
    assert!(
        !authority_text(&file).contains("[#dropme]"),
        "the write is durable"
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        BACKLOG_DOC,
        "no disk write is attempted while the editor holds the document"
    );
    assert!(ops_log(&file).contains("outcome=deferred"));
}

/// Without the binary-owned CLI policy (the response/closeout path), the
/// barrier refusal is unchanged: same message, no wait, no deferral type.
#[test]
fn the_retained_refusal_is_unchanged_outside_the_cli_policy() {
    let identity = "binaryownedcli-no-policy";
    let (_dir, file) = attached_document(BACKLOG_DOC, identity);
    let cli_file = file.clone();
    let result = std::thread::spawn(move || {
        agent_doc_element_backlog_io::with_backlog_command_effects(
            &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
            || agent_doc_element_backlog_io::backlog_cmd::remove(&cli_file, "stale item", true),
        )
    })
    .join()
    .unwrap();
    let err = result.expect_err("the unacknowledged write is retained");
    assert!(retained_cli_write_deferral(&err).is_none());
    let rendered = format!("{err:#}");
    assert!(
        agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&rendered),
        "{rendered}"
    );
    assert!(!ops_log(&file).contains("binary_owned_cli_write_retained_settled"));
}

/// A genuine refusal — the attached editor's replica is gone, so no live
/// replica will ever converge the write — still fails under the CLI policy.
#[test]
fn a_refused_write_without_a_live_replica_still_fails() {
    let identity = "binaryownedcli-refused";
    let (_dir, file) = attached_document(BACKLOG_DOC, identity);
    agent_doc_crdt_relay_io::deregister_replica_for_file(&file, identity).unwrap();
    let cli_file = file.clone();
    let result = std::thread::spawn(move || {
        agent_doc_element_backlog_io::with_backlog_command_effects(
            &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
            || {
                with_binary_owned_cli_write(Duration::from_secs(30), || {
                    agent_doc_element_backlog_io::backlog_cmd::remove(&cli_file, "stale item", true)
                })
            },
        )
    })
    .join()
    .unwrap();
    let err = result.expect_err("a write with no live replica to converge it must fail");
    assert!(
        retained_cli_write_deferral(&err).is_none(),
        "a refusal with no live replica is not a converging deferral: {err:#}"
    );
    assert!(
        !agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&format!(
            "{err:#}"
        )),
        "{err:#}"
    );
    assert!(!ops_log(&file).contains("binary_owned_cli_write_retained_settled"));
}

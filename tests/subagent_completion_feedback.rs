use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI, and
/// a controller launch by that CLI is production behavior, so every spawned
/// command opts in with this reason.
const CONTROLLER_LAUNCH_REASON: &str = "queue claim/repair CLI commands reach the project controller, which they launch as production does";

fn wait_for_path(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ops_log_len(doc: &std::path::Path) -> u64 {
    let ops = doc.parent().unwrap().join(".agent-doc/logs/ops.log");
    fs::metadata(ops)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn wait_for_background_queue_maintenance(doc: &std::path::Path, previous_ops_len: u64) {
    let ops = doc.parent().unwrap().join(".agent-doc/logs/ops.log");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let current = fs::read_to_string(&ops).unwrap_or_default();
        let appended = current.get(previous_ops_len as usize..).unwrap_or(&current);
        if appended.lines().any(|line| {
            line.contains("queue_maintenance_background_completed") && line.contains("mutated=")
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "detached queue maintenance did not complete: {appended}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn report_complete_holds_queue_apply_exclusion_across_the_whole_transaction() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let item = "prove report transaction exclusion";
    fs::write(
        &doc,
        format!(
            "---\nagent_doc_session: report-transaction\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append subagents -->\n### Re: prior — gpt-5\n\nDone.\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue subagents -->\n- {item}\n<!-- /agent:queue -->\n"
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let owner = "subagent:transaction";
    let run = |args: &[&str]| {
        let output = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&[
        "queue",
        "claim",
        doc.to_str().unwrap(),
        "--item",
        item,
        "--owner",
        owner,
    ]);
    run(&[
        "queue",
        "start",
        doc.to_str().unwrap(),
        "--item",
        item,
        "--owner",
        owner,
        "--generation",
        "1",
        "--worker-id",
        "worker:transaction",
    ]);

    let marker = dir.path().join("report-after-capture.marker");
    let release = dir.path().join("report-after-capture.release");
    let child = Command::new(binary)
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env(
            "AGENT_DOC_TEST_REPORT_COMPLETE_AFTER_CAPTURE_MARKER",
            &marker,
        )
        .env(
            "AGENT_DOC_TEST_REPORT_COMPLETE_AFTER_CAPTURE_RELEASE",
            &release,
        )
        .current_dir(dir.path())
        .args([
            "queue",
            "report-complete",
            "--prose-only",
            doc.to_str().unwrap(),
            "--item",
            item,
            "--generation",
            "1",
            "--response",
            "transaction response",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&marker);

    let paused = fs::read_to_string(&doc).unwrap();
    assert!(!paused.contains("transaction response"));
    assert!(
        !agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
            &doc.to_string_lossy(),
            "detached-maintenance-during-report",
            std::process::id(),
        )
        .unwrap(),
        "detached maintenance must coalesce while report-complete owns the transaction"
    );
    assert!(
        agent_doc_queue_io::queue_edit_owner::QueueEditGuard::try_acquire(&doc)
            .unwrap()
            .is_none()
    );

    fs::write(&release, b"release").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "report-complete failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fs::read_to_string(&doc)
            .unwrap()
            .contains("transaction response")
    );
}

#[test]
fn report_complete_apply_contention_fails_before_any_transaction_effect() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let item = "reject report before capture";
    fs::write(
        &doc,
        format!(
            "---\nagent_doc_session: report-contention\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append subagents -->\n### Re: prior — gpt-5\n\nDone.\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue subagents -->\n- {item}\n<!-- /agent:queue -->\n"
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let owner = "subagent:contention";
    for args in [
        vec![
            "queue",
            "claim",
            doc.to_str().unwrap(),
            "--item",
            item,
            "--owner",
            owner,
        ],
        vec![
            "queue",
            "start",
            doc.to_str().unwrap(),
            "--item",
            item,
            "--owner",
            owner,
            "--generation",
            "1",
            "--worker-id",
            "worker:contention",
        ],
    ] {
        let status = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let before = fs::read(&doc).unwrap();
    let _guard = agent_doc_queue_io::queue_edit_owner::QueueEditGuard::acquire(&doc).unwrap();
    let output = Command::new(binary)
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .current_dir(dir.path())
        .args([
            "queue",
            "report-complete",
            "--prose-only",
            doc.to_str().unwrap(),
            "--item",
            item,
            "--generation",
            "1",
            "--response",
            "must never be captured",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("queue mutation deferred"));
    assert_eq!(fs::read(&doc).unwrap(), before);
    assert!(
        agent_doc_queue_io::claim_release_receipt::load(&doc)
            .unwrap()
            .receipts
            .is_empty(),
        "contention must be rejected before response capture"
    );
}

#[test]
fn four_cli_reporters_serialize_without_losing_completion_feedback() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let items = ["finish sdk", "finish dev", "finish pane", "finish api"];
    let queue = items
        .iter()
        .map(|item| format!("- {item}\n"))
        .collect::<String>();
    fs::write(
        &doc,
        format!(
            concat!(
                "---\nagent_doc_session: multiprocess-completion-feedback\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append subagents -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "Operator edit that must survive.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "{}",
                "<!-- /agent:queue -->\n",
            ),
            queue,
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    for item in items {
        let owner = format!("subagent:{}", item.replace(' ', "-"));
        let worker = format!("worker:{}", item.replace(' ', "-"));
        let status = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args([
                "queue",
                "claim",
                doc.to_str().unwrap(),
                "--item",
                item,
                "--owner",
                &owner,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "claim failed for {item}: {status}");
        let status = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args([
                "queue",
                "start",
                doc.to_str().unwrap(),
                "--item",
                item,
                "--owner",
                &owner,
                "--generation",
                "1",
                "--worker-id",
                &worker,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "start failed for {item}: {status}");
    }

    let mut children = items
        .into_iter()
        .map(|item| {
            Command::new(binary)
                .env(
                    agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                    CONTROLLER_LAUNCH_REASON,
                )
                .current_dir(dir.path())
                .args([
                    "queue",
                    "report-complete",
                    "--prose-only",
                    doc.to_str().unwrap(),
                    "--item",
                    item,
                    "--generation",
                    "1",
                    "--response",
                    &format!("Completed {item}.\n\n- focused tests passed\n- branch pushed"),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    for child in children.drain(..) {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "reporter failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // `#completionlockload`: no wall-clock bound here. At load average ~122
    // on 2026-11-05 the reporters queued behind each other for longer than a
    // fixed budget while every owner was still handing off normally; that
    // measured the machine, not the lock. The wait's bounds (stall window,
    // ceiling) are proven structurally by the progress-lock unit tests in
    // `agent-doc-queue-io/src/completion_feedback_lock.rs`, and every reporter
    // above succeeding proves none of them refused while owners handed off.

    let content = fs::read_to_string(&doc).unwrap();
    assert!(content.contains("Operator edit that must survive."));
    assert_eq!(
        content.matches("### Re: Subagent completed").count(),
        items.len()
    );
    assert_eq!(
        content
            .matches("<!-- agent-doc-subagent-completion key=")
            .count(),
        items.len()
    );
    for item in items {
        assert_eq!(
            content.matches(item).count(),
            3,
            "queue + heading + worker response for {item}"
        );
        assert!(content.contains(&format!("> Completed {item}.")));
    }
    agent_doc_lint_io::validate_structure_on_content(&doc, &content)
        .expect("multi-process notices must pass the session-check integrity classifier");
}

#[test]
fn report_complete_keeps_component_markers_in_worker_response_inert_and_scoped() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let item = "diagnose game-sessions component corruption";
    fs::write(
        &doc,
        format!(
            concat!(
                "---\nagent_doc_session: completion-feedback-scope\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append subagents -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "- {item}\n",
                "<!-- /agent:queue -->\n",
            ),
            item = item,
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let owner = "subagent:game-sessions";
    let run = |args: &[&str]| {
        Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    let claim = run(&[
        "queue",
        "claim",
        doc.to_str().unwrap(),
        "--item",
        item,
        "--owner",
        owner,
    ]);
    assert!(
        claim.status.success(),
        "claim failed: {}",
        String::from_utf8_lossy(&claim.stderr)
    );
    let start = run(&[
        "queue",
        "start",
        doc.to_str().unwrap(),
        "--item",
        item,
        "--owner",
        owner,
        "--generation",
        "1",
        "--worker-id",
        "worker:game-sessions",
    ]);
    assert!(
        start.status.success(),
        "start failed: {}",
        String::from_utf8_lossy(&start.stderr)
    );

    let before = fs::read_to_string(&doc).unwrap();
    let response = concat!(
        "The live document has this repeated opening marker:\n\n",
        "<!-- agent:run -->\n",
        "- queued work\n",
        "<!-- agent:run -->\n\n",
        "The second marker must be the closing marker:\n",
        "<!-- /agent:run -->",
    );
    let report = run(&[
        "queue",
        "report-complete",
        "--prose-only",
        doc.to_str().unwrap(),
        "--item",
        item,
        "--generation",
        "1",
        "--response",
        response,
    ]);
    assert!(
        report.status.success(),
        "report failed: {}",
        String::from_utf8_lossy(&report.stderr)
    );

    let after = fs::read_to_string(&doc).unwrap();
    assert!(after.contains("> <!-- agent:run -->"));
    assert!(after.contains("> <!-- /agent:run -->"));
    assert!(after.contains("> The second marker must be the closing marker:"));
    let scope = agent_doc_element::component_scope::changed_component_scope(&before, &after)
        .expect("completion feedback must have a provable component scope");
    assert_eq!(
        scope.to_pairs(),
        vec![("exchange".to_string(), 0), ("queue".to_string(), 0)]
    );
}

#[test]
fn repair_completion_feedback_cli_migrates_bodyless_notices_without_replaying_closeout() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_session: completion-feedback-migration\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: Subagent completed — agent-doc: `subagent:sdk` returned work for queue item “finish sdk”; coordinator integration and closeout are pending.\n",
            "<!-- agent-doc-subagent-completion key=one -->\n",
            "### Re: Subagent completed — agent-doc: `subagent:dev` returned work for queue item “finish dev”; coordinator integration and closeout are pending.\n",
            "<!-- agent-doc-subagent-completion key=two -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- finish sdk\n",
            "- finish dev\n",
            "<!-- /agent:queue -->\n",
        ),
    )
    .unwrap();
    let before = fs::read_to_string(&doc).unwrap();
    assert!(
        agent_doc_lint_io::validate_structure_on_content(&doc, &before).is_err(),
        "fixture must reproduce the live bodyless-response integrity failure"
    );

    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let first = Command::new(binary)
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .current_dir(dir.path())
        .args(["queue", "repair-completion-feedback", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "migration failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stderr).contains("2 artifacts repaired"));

    let repaired = fs::read_to_string(&doc).unwrap();
    agent_doc_lint_io::validate_structure_on_content(&doc, &repaired)
        .expect("migrated notices must pass the actual session-check integrity classifier");
    assert_eq!(
        repaired
            .matches("### Re: Subagent completed — agent-doc:")
            .count(),
        2
    );
    assert_eq!(
        repaired
            .matches("Subagent completion feedback was recorded by agent-doc;")
            .count(),
        2
    );
    assert_eq!(repaired.matches("key=one").count(), 1);
    assert_eq!(repaired.matches("key=two").count(), 1);
    assert!(repaired.contains("- finish sdk"));
    assert!(repaired.contains("- finish dev"));

    let session_check = Command::new(binary)
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .current_dir(dir.path())
        .args(["session-check", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        session_check.status.success(),
        "the direct session-check command must accept the migrated document: stdout={} stderr={}",
        String::from_utf8_lossy(&session_check.stdout),
        String::from_utf8_lossy(&session_check.stderr),
    );

    let second = Command::new(binary)
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .current_dir(dir.path())
        .args(["queue", "repair-completion-feedback", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("0 artifacts repaired"));
    assert_eq!(fs::read_to_string(&doc).unwrap(), repaired);
}

/// `#stalecompletionrepair` through the real CLI: the dry run lists the cell a
/// pre-`#strikeretires` strike left behind and changes nothing; `--apply`
/// retires only that cell (operator text and the live row's cell stay, the
/// result still passes session-check), and a rerun is a no-op.
#[test]
fn repair_stale_completion_cli_is_dry_run_first_narrow_and_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let stale = agent_doc_document::subagent_completion::cell_with_response(
        "subagent:release_publish_patch",
        "release + publish",
        "stalekey",
        "Released 0.35.500.",
    )
    .unwrap();
    let live = agent_doc_document::subagent_completion::cell_with_response(
        "subagent:open",
        "finish dev",
        "livekey",
        "Still integrating.",
    )
    .unwrap();
    let original = format!(
        concat!(
            "---\nagent_doc_session: stale-completion-repair\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n\n",
            "{stale}\n\n{live}\n",
            "Operator edit that must survive.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- ~~release + publish~~\n",
            "- finish dev\n",
            "<!-- /agent:queue -->\n",
        ),
        stale = stale,
        live = live,
    );
    fs::write(&doc, &original).unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let run = |extra: &[&str]| {
        let mut args = vec!["queue", "repair-stale-completion", doc.to_str().unwrap()];
        args.extend_from_slice(extra);
        let output = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(&args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "repair failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (stdout, _) = run(&[]);
    assert!(
        stdout.contains("would retire completion cell key=stalekey"),
        "{stdout}"
    );
    assert!(!stdout.contains("livekey retire"), "{stdout}");
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        original,
        "dry run writes nothing"
    );

    let (stdout, _) = run(&["--apply"]);
    assert!(
        stdout.contains("retired completion cell key=stalekey"),
        "{stdout}"
    );
    let repaired = fs::read_to_string(&doc).unwrap();
    assert!(!repaired.contains(&stale), "{repaired}");
    assert!(repaired.contains(&live), "{repaired}");
    assert!(repaired.contains("### Re: prior — gpt-5\n\nDone.\n"));
    assert!(repaired.contains("Operator edit that must survive.\n"));
    assert!(repaired.contains("- ~~release + publish~~\n- finish dev\n"));
    agent_doc_lint_io::validate_structure_on_content(&doc, &repaired)
        .expect("the repaired document must pass the session-check integrity classifier");

    let (stdout, stderr) = run(&["--apply"]);
    assert!(!stdout.contains("retired completion cell"), "{stdout}");
    assert!(stderr.contains("0 stale completion cell(s)"), "{stderr}");
    assert_eq!(fs::read_to_string(&doc).unwrap(), repaired);
}

/// `#completioncellrestore` through the real CLI, on the 2026-11-05 shape:
/// two workers report through `report-complete`, then a closeout deletes both
/// completion cells while the rows stay reported. The dry run lists both and
/// writes nothing; `--apply` adds both back byte for byte from the saved
/// responses (operator text and queue rows unchanged, session-check still
/// passes); a rerun is a no-op, and the stale-completion repair keeps them.
#[test]
fn repair_completion_cells_cli_restores_deleted_cells_from_saved_responses() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let items = ["finish sdk", "finish dev"];
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_session: completion-cell-restore\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "Operator edit that must survive.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- finish sdk\n- finish dev\n",
            "<!-- /agent:queue -->\n",
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let run = |args: &[&str]| {
        let output = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let path = doc.to_str().unwrap();
    for item in items {
        let owner = format!("subagent:{}", item.replace(' ', "-"));
        let worker = format!("worker:{}", item.replace(' ', "-"));
        run(&["queue", "claim", path, "--item", item, "--owner", &owner]);
        run(&[
            "queue",
            "start",
            path,
            "--item",
            item,
            "--owner",
            &owner,
            "--generation",
            "1",
            "--worker-id",
            &worker,
        ]);
        run(&[
            "queue",
            "report-complete",
            "--prose-only",
            path,
            "--item",
            item,
            "--generation",
            "1",
            "--response",
            &format!("Completed {item}.\n\n- focused tests passed"),
        ]);
    }
    let reported = fs::read_to_string(&doc).unwrap();
    let lines = reported.lines().collect::<Vec<_>>();
    let cells = agent_doc_document::subagent_completion::completion_cell_line_ranges(&reported)
        .into_iter()
        .map(|range| lines[range].join("\n"))
        .collect::<Vec<_>>();
    assert_eq!(cells.len(), 2, "{reported}");

    // The closeout deletion: every cell gone, the reported rows kept.
    let deleted = agent_doc_document::subagent_completion::strip_completion_cells(&reported);
    assert!(!deleted.contains("### Re: Subagent completed"), "{deleted}");
    fs::write(&doc, &deleted).unwrap();

    let (stdout, stderr) = run(&["queue", "repair-completion-cells", path]);
    assert_eq!(
        stdout.matches("would restore completion cell key=").count(),
        2,
        "{stdout}\n{stderr}"
    );
    assert!(stderr.contains("dry run; rerun with --apply"), "{stderr}");
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        deleted,
        "dry run writes nothing"
    );

    let (stdout, _) = run(&["queue", "repair-completion-cells", path, "--apply"]);
    assert_eq!(
        stdout.matches("restored completion cell key=").count(),
        2,
        "{stdout}"
    );
    let restored = fs::read_to_string(&doc).unwrap();
    for cell in &cells {
        assert!(restored.contains(cell.as_str()), "{cell}\n---\n{restored}");
    }
    assert!(
        restored.contains("### Re: prior — gpt-5\n\nDone.\nOperator edit that must survive.\n")
    );
    let queue = |text: &str| {
        let start = text.find("<!-- agent:queue").unwrap();
        text[start..].to_string()
    };
    assert_eq!(
        queue(&restored),
        queue(&deleted),
        "queue rows are unchanged"
    );
    agent_doc_lint_io::validate_structure_on_content(&doc, &restored)
        .expect("the restored document must pass the session-check integrity classifier");

    let (stdout, stderr) = run(&["queue", "repair-completion-cells", path, "--apply"]);
    assert!(!stdout.contains("restored completion cell"), "{stdout}");
    assert!(
        stderr.contains("0 completion cell(s) to restore; 2 already present"),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), restored);

    let (stdout, _) = run(&["queue", "repair-stale-completion", path]);
    assert!(!stdout.contains("would retire completion cell"), "{stdout}");
}

fn git(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// A committed, baselined session document in a git repository: the shape the
/// coordinator's document has between turns, when the background workers
/// report back.
fn committed_session(dir: &std::path::Path, items: &[&str]) -> std::path::PathBuf {
    committed_session_with_response(dir, items, "Dispatched four subagents.\n")
}

/// [`committed_session`] whose last committed response body is `body`.
fn committed_session_with_response(
    dir: &std::path::Path,
    items: &[&str],
    body: &str,
) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let queue = items
        .iter()
        .map(|item| format!("- {item}\n"))
        .collect::<String>();
    let content = format!(
        concat!(
            "---\nagent_doc_session: completion-feedback-admission\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: dispatch — gpt-5 · 2026-10-14T22:00-04:00\n\n{}",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "{}",
            "<!-- /agent:queue -->\n",
        ),
        body, queue,
    );
    fs::write(&doc, &content).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["add", "session.md"]);
    git(dir, &["commit", "-q", "--no-verify", "-m", "initial"]);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    doc
}

fn run_agent_doc(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn assert_session_check_does_not_accuse_patchback(dir: &std::path::Path, doc: &std::path::Path) {
    let output = run_agent_doc(dir, &["session-check", doc.to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("direct response patchback")
            && !stderr.contains("direct response patchback"),
        "binary-owned completion feedback is not an agent's direct patchback: stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stdout.contains("uncommitted exchange changes")
            && !stderr.contains("uncommitted exchange changes"),
        "binary-owned completion feedback is admissible exchange drift: stdout={stdout} stderr={stderr} doc={}",
        fs::read_to_string(doc).unwrap()
    );
}

fn assert_preflight_admits(dir: &std::path::Path, doc: &std::path::Path, expected_cells: usize) {
    let previous_ops_len = ops_log_len(doc);
    let output = run_agent_doc(dir, &["preflight", doc.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "preflight must admit a document carrying binary completion feedback: stderr={stderr}"
    );
    assert!(!stderr.contains("failed closed"), "stderr={stderr}");
    assert!(!stderr.contains("refused to replay"), "stderr={stderr}");
    wait_for_background_queue_maintenance(doc, previous_ops_len);
    assert!(
        !dir.join(".agent-doc/repair-blocked").exists()
            || fs::read_dir(dir.join(".agent-doc/repair-blocked"))
                .unwrap()
                .next()
                .is_none(),
        "no payload may be quarantined as a blocked replay"
    );
    let content = fs::read_to_string(doc).unwrap();
    assert_eq!(
        content
            .matches("<!-- agent-doc-subagent-completion key=")
            .count(),
        expected_cells,
        "preflight must neither drop nor duplicate completion feedback: {content}"
    );
    assert_eq!(
        content.matches("### Re: Subagent completed").count(),
        expected_cells,
        "{content}"
    );
}

/// Binary-owned feedback is committed by the binary: the next preflight's
/// document-only auto-commit lands it in `HEAD`, so it never lingers as
/// uncommitted drift for a later recovery pass to misread.
fn assert_cells_committed(dir: &std::path::Path, expected_cells: usize) {
    let head = String::from_utf8(git(dir, &["show", "HEAD:session.md"]).stdout).unwrap();
    assert_eq!(
        head.matches("<!-- agent-doc-subagent-completion key=")
            .count(),
        expected_cells,
        "{head}"
    );
}

/// The live wedge (0.35.503): four `report-complete` calls on one document,
/// then the coordinator's next turn. Preflight's visible-response recovery read
/// the four binary cells as one orphaned agent response, and the replay guard
/// refused it because every heading normalized to the topic `subagent
/// completed`; session-check called the same cells a direct patchback.
#[test]
fn several_reports_then_preflight_admits_the_next_turn() {
    let dir = tempfile::tempdir().unwrap();
    let items = [
        "#gh-fix https://github.com/example/project/issues/262",
        "#gh-fix https://github.com/example/project/issues/264",
        "#gh-fix https://github.com/example/project/issues/263",
        "#gh-fix https://github.com/example/project/issues/261",
    ];
    let doc = committed_session(dir.path(), &items);
    for (index, item) in items.iter().enumerate() {
        let owner = format!("subagent:gh26{index}");
        let claim = run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                doc.to_str().unwrap(),
                "--item",
                item,
                "--owner",
                &owner,
            ],
        );
        assert!(
            claim.status.success(),
            "claim failed: {}",
            String::from_utf8_lossy(&claim.stderr)
        );
        let worker = format!("worker:gh26{index}");
        let start = run_agent_doc(
            dir.path(),
            &[
                "queue",
                "start",
                doc.to_str().unwrap(),
                "--item",
                item,
                "--owner",
                &owner,
                "--generation",
                "1",
                "--worker-id",
                &worker,
            ],
        );
        assert!(
            start.status.success(),
            "start failed: {}",
            String::from_utf8_lossy(&start.stderr)
        );
    }
    for item in items {
        let report = run_agent_doc(
            dir.path(),
            &[
                "queue",
                "report-complete",
                "--prose-only",
                doc.to_str().unwrap(),
                "--item",
                item,
                "--generation",
                "1",
                "--response",
                "Implemented the requested fix; focused tests passed.",
            ],
        );
        assert!(
            report.status.success(),
            "report-complete failed: {}",
            String::from_utf8_lossy(&report.stderr)
        );
    }
    let reported = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        reported.matches("### Re: Subagent completed").count(),
        items.len()
    );

    assert_session_check_does_not_accuse_patchback(dir.path(), &doc);
    assert_preflight_admits(dir.path(), &doc, items.len());
    assert_cells_committed(dir.path(), items.len());
    assert_session_check_does_not_accuse_patchback(dir.path(), &doc);
}

/// `#subagentcompletioncell`: a completion cell appended under a response
/// whose last line is its closing question is separated from it by a blank
/// line, so commit-time prompt canonicalization never reads that question as
/// an answered operator prompt and prefixes it with `❯`
/// (game-sessions.md, 2026-10-30).
#[test]
fn a_completion_cell_under_a_closing_question_is_separated_and_never_prefixed() {
    let dir = tempfile::tempdir().unwrap();
    let item = "#gh-fix https://github.com/example/project/issues/741";
    let question = "Should the PR path proceed, or remain on hold?";
    let doc = committed_session_with_response(
        dir.path(),
        &[item],
        &format!("The worker is running.\n\n{question}\n"),
    );
    let owner = "subagent:gh741";
    for args in [
        vec![
            "queue",
            "claim",
            doc.to_str().unwrap(),
            "--item",
            item,
            "--owner",
            owner,
        ],
        vec![
            "queue",
            "start",
            doc.to_str().unwrap(),
            "--item",
            item,
            "--owner",
            owner,
            "--generation",
            "1",
            "--worker-id",
            "worker:gh741",
        ],
        vec![
            "queue",
            "report-complete",
            "--prose-only",
            doc.to_str().unwrap(),
            "--item",
            item,
            "--generation",
            "1",
            "--response",
            "Implemented the requested fix; focused tests passed.",
        ],
    ] {
        assert_ok(&run_agent_doc(dir.path(), &args), args[1]);
    }
    let separated = format!("\n{question}\n\n### Re: Subagent completed");
    let reported = fs::read_to_string(&doc).unwrap();
    assert!(reported.contains(&separated), "{reported}");

    assert_preflight_admits(dir.path(), &doc, 1);
    assert_cells_committed(dir.path(), 1);
    let head = String::from_utf8(git(dir.path(), &["show", "HEAD:session.md"]).stdout).unwrap();
    assert!(head.contains(&separated), "{head}");
    assert!(!head.contains(&format!("❯ {question}")), "{head}");
    assert!(
        !fs::read_to_string(&doc)
            .unwrap()
            .contains(&format!("❯ {question}"))
    );
}

/// Recovery for a document already wedged by the old heading grammar: the exact
/// four-heading payload the 0.35.503 binary wrote, left uncommitted, must be
/// admitted without the false duplicate-topic refusal.
#[test]
fn preflight_admits_a_document_wedged_by_old_format_completion_feedback() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_session(dir.path(), &["finish"]);
    let committed = fs::read_to_string(&doc).unwrap();
    let mut cells = String::new();
    for (owner, issue) in [
        ("gh262", "262"),
        ("gh264", "264"),
        ("gh263", "263"),
        ("gh261", "261"),
    ] {
        // The 0.35.503 writer put a blank separator before every cell.
        cells.push_str(&format!(
            concat!(
                "\n",
                "### Re: Subagent completed — agent-doc: `subagent:{owner}` returned work for queue item “#gh-fix https://github.com/example/project/issues/{issue}”; coordinator integration and closeout are pending.\n",
                "Subagent completion feedback was recorded by agent-doc; the claim remains active pending coordinator integration and closeout.\n",
                "<!-- agent-doc-subagent-completion key=old{issue} -->\n",
            ),
            owner = owner,
            issue = issue,
        ));
    }
    let wedged = committed.replacen(
        "<!-- agent:boundary:keep -->",
        &format!("{cells}<!-- agent:boundary:keep -->"),
        1,
    );
    assert_ne!(wedged, committed);
    fs::write(&doc, &wedged).unwrap();

    // The live refusal came from orphaned-response recovery: it read the four
    // cells as one visible response and the replay guard refused the payload
    // as "repeated the assistant response heading `subagent completed`".
    let repair = run_agent_doc(dir.path(), &["repair", doc.to_str().unwrap()]);
    let repair_stderr = String::from_utf8_lossy(&repair.stderr);
    assert!(
        repair.status.success(),
        "recovery must not treat binary completion feedback as an orphaned response: {repair_stderr}"
    );
    assert!(
        !repair_stderr.contains("refused to replay")
            && !repair_stderr.contains("Found orphaned response"),
        "{repair_stderr}"
    );
    assert_eq!(fs::read_to_string(&doc).unwrap(), wedged);

    assert_session_check_does_not_accuse_patchback(dir.path(), &doc);
    assert_preflight_admits(dir.path(), &doc, 4);
    assert_cells_committed(dir.path(), 4);
    assert_session_check_does_not_accuse_patchback(dir.path(), &doc);
}

/// A committed, baselined go-mode `agent:run` whose `agent:backlog` mirrors
/// into it (`queue` attribute): the shape of the live `agent-doc-bugs.md`
/// session in `#queuerowdup`.
fn committed_run_session(dir: &std::path::Path, run_rows: &[&str]) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let rows = run_rows
        .iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>();
    let content = format!(
        concat!(
            "---\nagent_doc_session: queue-row-dup\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: dispatch — gpt-5 · 2026-10-27T08:00-04:00\n\nDispatched two subagents.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Run <!-- agent:run subagents priority go -->\n",
            "{}",
            "<!-- /agent:run -->\n\n",
            "## Backlog <!-- agent:backlog queue priority -->\n",
            "- [ ] [#alpha] fix the alpha defect\n",
            "- [ ] [#beta] fix the beta defect\n",
            "<!-- /agent:backlog -->\n",
        ),
        rows,
    );
    fs::write(&doc, &content).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["add", "session.md"]);
    git(dir, &["commit", "-q", "--no-verify", "-m", "initial"]);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    doc
}

#[test]
fn report_complete_joins_repository_provenance_to_the_exact_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_session(dir.path(), &["repository work"]);
    let path = doc.to_str().unwrap();
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                "repository work",
                "--owner",
                "subagent:repo",
            ],
        ),
        "claim",
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "start",
                path,
                "--item",
                "repository work",
                "--owner",
                "subagent:repo",
                "--generation",
                "1",
                "--worker-id",
                "worker:repo",
            ],
        ),
        "start",
    );

    let unclassified = run_agent_doc(
        dir.path(),
        &[
            "queue",
            "report-complete",
            path,
            "--item",
            "repository work",
            "--generation",
            "1",
            "--response",
            "implemented",
        ],
    );
    assert!(!unclassified.status.success());
    assert!(String::from_utf8_lossy(&unclassified.stderr).contains("exactly one result class"));

    let source = String::from_utf8(git(dir.path(), &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    let sink_ref = String::from_utf8(git(dir.path(), &["branch", "--show-current"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    let report = run_agent_doc(
        dir.path(),
        &[
            "queue",
            "report-complete",
            path,
            "--item",
            "repository work",
            "--generation",
            "1",
            "--source-repo",
            dir.path().to_str().unwrap(),
            "--source-sha",
            &source,
            "--sink-repo",
            dir.path().to_str().unwrap(),
            "--sink-ref",
            &sink_ref,
            "--response",
            "implemented and tested",
        ],
    );
    assert_ok(&report, "report repository completion");
    assert!(String::from_utf8_lossy(&report.stderr).contains("is not integrated"));
    // `#repositoryreportcell`: a repository result writes the same
    // binary-owned, digest-attested completion cell into `agent:exchange` as a
    // prose result, so the coordinator sees every worker result in exchange.
    let written = fs::read_to_string(&doc).unwrap();
    let evidence = agent_doc_document::subagent_completion::completion_evidence(&written);
    assert_eq!(evidence.len(), 1, "{written}");
    assert_eq!(evidence[0].owner, "subagent:repo");
    assert_eq!(evidence[0].item, "repository work");
    let exchange_end = written.find("<!-- /agent:exchange -->").unwrap();
    let cell_at = written
        .find("### Re: Subagent completed: `subagent:repo` returned work for queue item “repository work” — agent-doc")
        .expect("repository report must write its completion cell");
    assert!(
        cell_at < exchange_end,
        "the cell belongs inside agent:exchange: {written}"
    );
    assert_eq!(
        agent_doc_document::subagent_completion::completion_response(&written, &evidence[0].key)
            .as_deref(),
        Some("implemented and tested"),
        "{written}"
    );
    let lineage = run_agent_doc(
        dir.path(),
        &[
            "queue",
            "lineage",
            path,
            "--item",
            "repository work",
            "--generation",
            "1",
            "--json",
        ],
    );
    assert_ok(&lineage, "query lineage");
    let json: serde_json::Value = serde_json::from_slice(&lineage.stdout).unwrap();
    assert_eq!(json["attempts"][0]["source_sha"], source);
    assert_eq!(
        json["attempts"][0]["source_repo_path"],
        dir.path().to_string_lossy().as_ref()
    );
    assert_eq!(
        json["attempts"][0]["sink_repo_path"],
        dir.path().to_string_lossy().as_ref()
    );
    assert_eq!(json["attempts"][0]["state"], "committed");
}

/// The `agent:run` rows of `doc`.
fn run_rows(doc: &std::path::Path) -> Vec<String> {
    let content = fs::read_to_string(doc).unwrap();
    let start = content.find("<!-- agent:run ").unwrap();
    let body_start = start + content[start..].find('\n').unwrap() + 1;
    let end = content.find("<!-- /agent:run -->").unwrap();
    content[body_start..end]
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn assert_ok(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn preflight_json(dir: &std::path::Path, doc: &std::path::Path) -> serde_json::Value {
    let previous_ops_len = ops_log_len(doc);
    let output = run_agent_doc(dir, &["preflight", doc.to_str().unwrap()]);
    assert_ok(&output, "preflight");
    wait_for_background_queue_maintenance(doc, previous_ops_len);
    // The production admission result intentionally precedes comprehensive
    // projection. These assertions inspect the settled tree, so take a pure
    // probe after the detached receipt rather than opening another cycle.
    let settled = run_agent_doc(dir, &["preflight", "--probe", doc.to_str().unwrap()]);
    assert_ok(&settled, "settled preflight probe");
    serde_json::from_slice(&settled.stdout).unwrap_or_else(|err| {
        panic!(
            "preflight stdout is not JSON ({err}): {}",
            String::from_utf8_lossy(&settled.stdout)
        )
    })
}

fn rows_naming(rows: &[String], id: &str) -> Vec<String> {
    let tag = format!("[#{id}]");
    rows.iter()
        .filter(|row| row.contains(&tag))
        .cloned()
        .collect()
}

/// `#queuerowdup` (agent-doc-bugs.md, 2026-10-27): the live `agent:run` showed
/// `- ☑️ [#subagent:clearqueuedciflake#1] do [#clearqueuedciflake]` beside a
/// plain `- do [#clearqueuedciflake]`. The binary writers that touch an
/// id-backed head across a subagent attempt (the backlog mirror, `queue
/// claim` / `start` / `report-complete`, the preflight lifecycle projection,
/// and a later mirror pass) must each key the head by its work identity, never
/// by its rendered `do [#id]` prefix, so none of them adds a second row. A
/// plain copy that is NOT committed is a re-emit, and preflight collapses it
/// into the marked row.
#[test]
fn a_claimed_mirrored_id_head_stays_one_row_through_its_whole_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_run_session(dir.path(), &[]);
    let path = doc.to_str().unwrap();
    let ids = ["alpha", "beta"];
    let assert_single = |stage: &str, marker: Option<&str>| {
        let rows = run_rows(&doc);
        for id in ids {
            let named = rows_naming(&rows, id);
            assert_eq!(named.len(), 1, "{stage}: one row for #{id}: {rows:#?}");
            if let Some(marker) = marker {
                let expected = format!("- ### {marker} [#subagent:{id}#1] do [#{id}]");
                assert_eq!(named[0], expected, "{stage}: {rows:#?}");
            }
        }
    };

    assert_ok(
        &run_agent_doc(dir.path(), &["queue", "sync", path]),
        "queue sync",
    );
    assert_single("mirrored", None);
    for id in ids {
        let item = format!("do [#{id}]");
        let owner = format!("subagent:{id}");
        assert_ok(
            &run_agent_doc(
                dir.path(),
                &["queue", "claim", path, "--item", &item, "--owner", &owner],
            ),
            "queue claim",
        );
        assert_ok(
            &run_agent_doc(
                dir.path(),
                &[
                    "queue",
                    "start",
                    path,
                    "--item",
                    &item,
                    "--owner",
                    &owner,
                    "--generation",
                    "1",
                    "--worker-id",
                    &format!("worker:{id}"),
                ],
            ),
            "queue start",
        );
    }
    assert_single("started", Some("🚧"));
    preflight_json(dir.path(), &doc);
    assert_single("preflight while running", Some("🚧"));
    for id in ids {
        let report = run_agent_doc(
            dir.path(),
            &[
                "queue",
                "report-complete",
                "--prose-only",
                path,
                "--item",
                &format!("do [#{id}]"),
                "--generation",
                "1",
                "--response",
                &format!("Implemented and tested the {id} fix; branch fix/{id} is pushed."),
            ],
        );
        assert_ok(&report, "queue report-complete");
        let stderr = String::from_utf8_lossy(&report.stderr);
        assert!(stderr.contains("awaiting integration."), "{stderr}");
        assert!(
            stderr.contains(
                "side-worktree commit, or published side branch is not terminal completion"
            ),
            "report-complete must explain that a worker handback is integration debt: {stderr}"
        );
        assert!(
            stderr.contains("verify the authorized delivery boundary and commit reachability"),
            "report-complete must give the integration-proof next step: {stderr}"
        );
        assert!(
            stderr.contains(&format!("agent-doc queue consume {path} --ack-id {id}")),
            "an id-backed report must say how to strike its row while keeping tracked work open: {stderr}"
        );
    }
    assert_single("reported", Some("☑️"));
    preflight_json(dir.path(), &doc);
    assert_single("preflight after report", Some("☑️"));
    assert_ok(
        &run_agent_doc(dir.path(), &["queue", "sync", path]),
        "queue sync after report",
    );
    assert_single("re-mirrored", Some("☑️"));

    // The preflight after the report opened a coordinator cycle for the
    // reported head. Close it before the next edit, as a coordinator closeout
    // would: an open `preflight_started` cycle plus a fresh queue edit is a
    // live turn the next preflight must not steal. (Before `#backlogrowmarker`
    // this passed only because the `☑️` painted onto the backlog rows left the
    // snapshot ahead of HEAD, so interrupted-cycle recovery committed it.)
    assert_ok(
        &run_agent_doc(dir.path(), &["commit", path]),
        "close the coordinator cycle",
    );
    // An uncommitted plain copy of each marked head (the shape of a stale
    // editor or CRDT re-emit) exceeds the committed multiplicity.
    let content = fs::read_to_string(&doc).unwrap();
    let with_copies = content.replacen(
        "<!-- /agent:run -->",
        "- do [#alpha]\n- do [#beta]\n<!-- /agent:run -->",
        1,
    );
    assert_ne!(with_copies, content);
    fs::write(&doc, &with_copies).unwrap();
    preflight_json(dir.path(), &doc);
    assert_single("preflight after an uncommitted plain copy", Some("☑️"));
}

/// `#queuerowdup`: the dangerous half of the report. A plain `do [#id]` twin
/// the operator COMMITTED beside a row whose worker is still running is a
/// lawful authored repeat, so it survives convergence. Preflight must still
/// key it by work identity: the twin is neither an inline-drainable selected
/// head nor a fresh subagent dispatch, because either would run the claimed
/// work a second time while its worker is live.
#[test]
fn a_committed_plain_twin_of_a_running_claim_is_never_selected_or_dispatched() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_run_session(dir.path(), &["- do [#alpha]"]);
    let path = doc.to_str().unwrap();
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
            ],
        ),
        "queue claim",
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "start",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
                "--generation",
                "1",
                "--worker-id",
                "worker:alpha",
            ],
        ),
        "queue start",
    );
    let content = fs::read_to_string(&doc).unwrap();
    assert!(
        content.contains("- 🚧 [#subagent:alpha#1] do [#alpha]\n"),
        "{content}"
    );
    let with_twin = content.replacen(
        "<!-- /agent:run -->",
        "- do [#alpha]\n<!-- /agent:run -->",
        1,
    );
    fs::write(&doc, &with_twin).unwrap();
    git(dir.path(), &["add", "session.md"]);
    git(
        dir.path(),
        &["commit", "-q", "--no-verify", "-m", "operator twin"],
    );
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &with_twin,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();

    let state = preflight_json(dir.path(), &doc);
    let named = rows_naming(&run_rows(&doc), "alpha");
    assert_eq!(
        named.len(),
        2,
        "the committed twin is an authored repeat and must survive, or this test proves nothing: {named:#?}"
    );
    let strings = |key: &str| -> Vec<String> {
        state
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let selected = strings("selected_queue_prompts");
    assert!(
        selected
            .iter()
            .all(|prompt| !prompt.contains("do [#alpha]")),
        "a twin of running work must never be an inline-drainable head: {selected:?}\n{state:#}"
    );
    let dispatch = strings("queue_subagent_dispatch");
    // Preflight mirrored the unclaimed `#beta` and offers it for dispatch: the
    // dispatch list is live in this fixture, so its silence on `#alpha` is
    // evidence, not an omitted key.
    assert!(
        dispatch.iter().any(|entry| entry.contains("do [#beta]")),
        "{state:#}"
    );
    assert!(
        dispatch.iter().all(|entry| !entry.contains("do [#alpha]")),
        "a twin of running work must never be dispatched again: {dispatch:?}"
    );
    assert_eq!(
        strings("queue_prompts")
            .iter()
            .filter(|prompt| prompt.contains("do [#alpha]"))
            .count(),
        2,
        "both alpha rows are queue prompts; only their identity keeps them out: {state:#}"
    );
    assert_eq!(
        state.get("queue_drainable_head_count"),
        Some(&serde_json::json!(0)),
        "{state:#}"
    );
}

/// `#lifecyclewordfusion` (haiven-dev sdk-dev.md, 2026-11-03): after dispatch
/// the running row read `📥 running🚧 do [#q-c2010d98af04]`. The state word
/// fused to its glyph hid the row from its own running attempt, so
/// `queue report-complete` printed "no subagent claim" and wrote nothing, and
/// the coordinator struck the row by echo text instead. The fused token is
/// projection: the report lands on the exact attempt and the repaint drops
/// the word.
#[test]
fn report_complete_finds_the_attempt_on_a_row_whose_state_word_was_fused() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let parent = "ADR 0046 approvers";
    let leaf = "do [#q-c2010d98af04]";
    fs::write(
        &doc,
        format!(
            concat!(
                "---\nagent_doc_session: fused-state-word\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append subagents -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "- {parent}\n",
                "  - {leaf}\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#q-c2010d98af04] Add Dillon + Eddie.\n",
                "<!-- /agent:backlog -->\n",
            ),
            parent = parent,
            leaf = leaf,
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let run = |args: &[&str]| {
        let output = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let path = doc.to_str().unwrap();
    let vector = format!("{parent}: {leaf}");
    run(&[
        "queue",
        "claim",
        path,
        "--item",
        &vector,
        "--owner",
        "subagent:dillon",
    ]);
    run(&[
        "queue",
        "start",
        path,
        "--item",
        &vector,
        "--owner",
        "subagent:dillon",
        "--generation",
        "1",
        "--worker-id",
        "worker:dillon",
    ]);
    let started = fs::read_to_string(&doc).unwrap();
    assert!(
        started.contains("  - 🚧 [#subagent:dillon#1] do [#q-c2010d98af04]\n"),
        "{started}"
    );
    // The live corruption: the state word fused in front of its glyph and the
    // owner annotation lost.
    fs::write(
        &doc,
        started.replace(
            "  - 🚧 [#subagent:dillon#1] do [#q-c2010d98af04]\n",
            "  - 📥 running🚧 do [#q-c2010d98af04]\n",
        ),
    )
    .unwrap();

    let stderr = run(&[
        "queue",
        "report-complete",
        "--prose-only",
        path,
        "--item",
        &vector,
        "--generation",
        "1",
        "--response",
        "Recorded the decision.",
    ]);
    assert!(
        stderr.contains("subagent completion feedback accepted"),
        "the report must land on the running attempt: {stderr}"
    );
    let reported = fs::read_to_string(&doc).unwrap();
    assert!(
        reported.contains("  - ☑️ [#subagent:dillon#1] do [#q-c2010d98af04]\n"),
        "{reported}"
    );
    assert!(!reported.contains("running🚧"), "{reported}");
}

/// `#reportcompleteselector` (agent-doc-bugs.md, 2026-11-04): a row whose text
/// holds `: ` (`[#lazilyreleasedecision] Decide … (#x): push it …`) was
/// claimed by its full text, which keys as a free-text nest vector. `queue
/// report-complete --item '#lazilyreleasedecision'` then printed "no subagent
/// claim" and wrote nothing; only the full text worked. An id selector names
/// the claimed row whichever selector the claim used.
#[test]
fn report_complete_resolves_an_id_selector_to_a_row_claimed_by_full_text() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    let row = "[#lazilyreleasedecision] Decide for lazily-rs 29fde5a8c6 (#lazilysqlitestoreopen): push it to lazily-rs main";
    fs::write(
        &doc,
        format!(
            concat!(
                "---\nagent_doc_session: report-complete-selector\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append subagents -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "- {row}\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#lazilyreleasedecision] Decide the lazily release.\n",
                "<!-- /agent:backlog -->\n",
            ),
            row = row,
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let run = |args: &[&str]| {
        let output = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let ok = |args: &[&str]| {
        let (success, stderr) = run(args);
        assert!(success, "{args:?} failed: {stderr}");
        stderr
    };
    let path = doc.to_str().unwrap();
    ok(&[
        "queue",
        "claim",
        path,
        "--item",
        row,
        "--owner",
        "subagent:lazilyrelease",
    ]);
    ok(&[
        "queue",
        "start",
        path,
        "--item",
        row,
        "--owner",
        "subagent:lazilyrelease",
        "--generation",
        "1",
        "--worker-id",
        "worker:lazilyrelease",
    ]);

    for selector in [
        "#lazilyreleasedecision",
        "lazilyreleasedecision",
        "do [#lazilyreleasedecision]",
    ] {
        let stderr = ok(&[
            "queue",
            "report-complete",
            "--prose-only",
            path,
            "--item",
            selector,
            "--generation",
            "1",
            "--response",
            "Pushed 29fde5a8c6; the release stays the operator's call.",
        ]);
        assert!(
            !stderr.contains("no subagent claim"),
            "{selector:?} must resolve the full-text claim: {stderr}"
        );
        assert!(
            stderr.contains("subagent completion feedback accepted")
                || stderr.contains("subagent completion feedback already present"),
            "{selector:?} must report the claimed attempt: {stderr}"
        );
    }
    let reported = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        reported
            .matches("Pushed 29fde5a8c6; the release stays the operator's call.")
            .count(),
        1,
        "one completion cell for the one attempt: {reported}"
    );
    assert!(
        reported.contains("☑️ [#subagent:lazilyrelease#1] [#lazilyreleasedecision] Decide"),
        "{reported}"
    );
}

/// `#reportcompleteselector`: an id that two claims carry (the same bare
/// `#tag` under two parents is two nest-vector claims, `#nestlistlineage`)
/// refuses with both candidates instead of reporting either attempt.
#[test]
fn report_complete_refuses_an_ambiguous_id_selector_with_the_candidates() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_session: report-complete-ambiguous\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- Release prep\n",
            "  - #rebase\n",
            "- Hotfix branch\n",
            "  - #rebase\n",
            "<!-- /agent:queue -->\n",
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let run = |args: &[&str]| {
        let output = Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    let path = doc.to_str().unwrap();
    for (vector, owner) in [
        ("Release prep: #rebase", "subagent:prep"),
        ("Hotfix branch: #rebase", "subagent:hotfix"),
    ] {
        let (success, stderr) = run(&["queue", "claim", path, "--item", vector, "--owner", owner]);
        assert!(success, "claim {vector:?} failed: {stderr}");
        let worker = format!("worker:{owner}");
        let (success, stderr) = run(&[
            "queue",
            "start",
            path,
            "--item",
            vector,
            "--owner",
            owner,
            "--generation",
            "1",
            "--worker-id",
            &worker,
        ]);
        assert!(success, "start {vector:?} failed: {stderr}");
    }
    let before = fs::read_to_string(&doc).unwrap();
    let (success, stderr) = run(&[
        "queue",
        "report-complete",
        "--prose-only",
        path,
        "--item",
        "#rebase",
        "--generation",
        "1",
        "--response",
        "Rebased.",
    ]);
    assert!(!success, "an ambiguous id must refuse: {stderr}");
    assert!(stderr.contains("Release prep: #rebase"), "{stderr}");
    assert!(stderr.contains("Hotfix branch: #rebase"), "{stderr}");
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        before,
        "a refused selector writes nothing"
    );
}

/// `#reportcompletenomatch`: `queue report-complete` exited 0 when its item
/// matched no claim, printing only "no subagent claim", and that silent
/// success let a lost worker result go unnoticed. A report that matches no
/// subagent claim now fails, names the document's candidate rows and every
/// claim with its owner, generation, and state, and writes nothing.
#[test]
fn report_complete_without_a_matching_subagent_claim_fails_and_names_candidates() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_session: report-complete-no-match\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- finish sdk\n",
            "- finish dev\n",
            "<!-- /agent:queue -->\n",
        ),
    )
    .unwrap();
    let binary = env!("CARGO_BIN_EXE_agent-doc");
    let run = |args: &[&str]| {
        Command::new(binary)
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    let path = doc.to_str().unwrap();
    for args in [
        &[
            "queue",
            "claim",
            path,
            "--item",
            "finish sdk",
            "--owner",
            "subagent:sdk",
        ][..],
        &[
            "queue",
            "start",
            path,
            "--item",
            "finish sdk",
            "--owner",
            "subagent:sdk",
            "--generation",
            "1",
            "--worker-id",
            "worker:sdk",
        ][..],
        &[
            "queue",
            "claim",
            path,
            "--item",
            "finish dev",
            "--owner",
            "operator:brian",
        ][..],
    ] {
        let output = run(args);
        assert!(
            output.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let before = fs::read_to_string(&doc).unwrap();
    let report = |item: &str| {
        run(&[
            "queue",
            "report-complete",
            "--prose-only",
            path,
            "--item",
            item,
            "--generation",
            "1",
            "--response",
            "Implemented the fix.",
        ])
    };

    let unmatched = report("finish nothing");
    let stderr = String::from_utf8_lossy(&unmatched.stderr);
    assert!(
        !unmatched.status.success(),
        "an unmatched report must not exit 0: {stderr}"
    );
    assert!(
        stderr.contains("matched no subagent claim for \"finish nothing\" generation 1"),
        "{stderr}"
    );
    assert!(
        stderr.contains("the worker result is NOT recorded"),
        "{stderr}"
    );
    assert!(
        stderr.contains("candidate rows (2):")
            && stderr.contains("  - \"finish sdk\"")
            && stderr.contains("  - \"finish dev\""),
        "the refusal names every candidate row: {stderr}"
    );
    assert!(
        stderr.contains(
            "\"finish sdk\" owner=subagent:sdk generation=1 state=running worker=worker:sdk"
        ) && stderr.contains("\"finish dev\" owner=operator:brian generation=1 state=claimed"),
        "the refusal names every claim with owner, generation, and state: {stderr}"
    );

    let foreign = report("finish dev");
    let stderr = String::from_utf8_lossy(&foreign.stderr);
    assert!(
        !foreign.status.success(),
        "a report on a non-subagent claim must not exit 0: {stderr}"
    );
    assert!(
        stderr.contains("held by operator:brian generation 1, not a `subagent:` owner"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&doc).unwrap(),
        before,
        "a refused report writes nothing"
    );
}

/// `#subagentworkpool`, end to end through the CLI: `report-complete` prints
/// the pool refill with the unstarted claim's spawn first and the next head's
/// exact claim / brief / start commands; `queue freeze` frees a slot and
/// emits the stopped row's fresh retry; `queue pool --json` reports the same
/// projection.
#[test]
fn report_complete_and_freeze_print_the_work_pool_refill() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    let doc = dir.path().join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_session: pool-refill\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents=2 -->\n",
            "- task one\n- task two\n- task three\n- task four\n",
            "<!-- /agent:queue -->\n",
        ),
    )
    .unwrap();
    let path = doc.to_str().unwrap().to_string();
    let base = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let run = |at: u64, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_agent-doc"))
            .env(
                agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
                CONTROLLER_LAUNCH_REASON,
            )
            .env("AGENT_DOC_TEST_QUEUE_CLAIM_NOW_SECS", at.to_string())
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    let ok = |output: std::process::Output, what: &str| {
        assert!(
            output.status.success(),
            "{what}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    ok(
        run(
            base,
            &[
                "queue",
                "claim",
                &path,
                "--item",
                "task one",
                "--owner",
                "subagent:one",
            ],
        ),
        "claim one",
    );
    ok(
        run(
            base,
            &[
                "queue",
                "start",
                &path,
                "--item",
                "task one",
                "--owner",
                "subagent:one",
                "--generation",
                "1",
                "--worker-id",
                "worker:one",
            ],
        ),
        "start one",
    );
    // Claimed, never spawned: the Grok miss.
    ok(
        run(
            base,
            &[
                "queue",
                "claim",
                &path,
                "--item",
                "task two",
                "--owner",
                "subagent:two",
            ],
        ),
        "claim two",
    );

    let later = base + 60;
    let report = ok(
        run(
            later,
            &[
                "queue",
                "report-complete",
                "--prose-only",
                &path,
                "--item",
                "task one",
                "--generation",
                "1",
                "--response",
                "Completed task one.",
            ],
        ),
        "report one",
    );
    assert!(
        report.contains("[queue] subagent work pool (#subagentworkpool, trigger=report_complete)"),
        "{report}"
    );
    let urgent = report.find("URGENT").unwrap_or_else(|| panic!("{report}"));
    let refill = report
        .find("Refill a free pool slot")
        .unwrap_or_else(|| panic!("{report}"));
    assert!(urgent < refill, "{report}");
    assert!(report.contains("\"task two\""), "{report}");
    assert!(report.contains("agent-doc queue start"), "{report}");
    assert!(
        report.contains("--generation 1 --worker-id <worker-id>"),
        "{report}"
    );
    assert!(report.contains("\"task three\""), "{report}");
    assert!(report.contains("agent-doc queue claim"), "{report}");
    assert!(report.contains("agent-doc queue brief"), "{report}");
    assert!(report.contains("1 more head(s) wait"), "{report}");

    // Bind the unstarted claim, fill the pool, then freeze one worker.
    ok(
        run(
            later,
            &[
                "queue",
                "start",
                &path,
                "--item",
                "task two",
                "--owner",
                "subagent:two",
                "--generation",
                "1",
                "--worker-id",
                "worker:two",
            ],
        ),
        "start two",
    );
    ok(
        run(
            later,
            &[
                "queue",
                "claim",
                &path,
                "--item",
                "task three",
                "--owner",
                "subagent:three",
            ],
        ),
        "claim three",
    );
    ok(
        run(
            later,
            &[
                "queue",
                "start",
                &path,
                "--item",
                "task three",
                "--owner",
                "subagent:three",
                "--generation",
                "1",
                "--worker-id",
                "worker:three",
            ],
        ),
        "start three",
    );
    let freeze = ok(
        run(
            later,
            &[
                "queue",
                "freeze",
                &path,
                "--item",
                "task two",
                "--generation",
                "1",
                "--reason",
                "ended without a result",
            ],
        ),
        "freeze two",
    );
    assert!(freeze.contains("trigger=frozen"), "{freeze}");
    assert!(freeze.contains("\"task two\""), "{freeze}");
    assert!(
        freeze.contains("use its exact claim / brief / start commands for a fresh generation"),
        "{freeze}"
    );
    assert!(
        freeze.contains("agent-doc queue claim")
            && freeze.contains("agent-doc queue brief")
            && freeze.contains("agent-doc queue start"),
        "{freeze}"
    );

    let pool = run(later, &["queue", "pool", &path, "--json"]);
    assert!(pool.status.success());
    let pool: serde_json::Value = serde_json::from_slice(&pool.stdout).unwrap();
    assert_eq!(pool["pools"][0]["occupancy"]["size"], 2, "{pool}");
    assert_eq!(
        pool["pools"][0]["occupancy"]["source"], "queue_cap",
        "{pool}"
    );
    assert_eq!(pool["pools"][0]["occupancy"]["frozen"], 0, "{pool}");
    assert_eq!(pool["pools"][0]["occupancy"]["free"], 1, "{pool}");
    assert_eq!(pool["next_heads"][0]["item"], "task two", "{pool}");

    // Failed automatic continuation authorizes a replacement generation.
    let claims = run(later, &["queue", "claims", &path]);
    assert!(claims.status.success());
    let claims: serde_json::Value = serde_json::from_slice(&claims.stdout).unwrap();
    let frozen = claims
        .as_array()
        .unwrap()
        .iter()
        .find(|claim| claim["item_text"] == "task two")
        .unwrap();
    assert_eq!(frozen["state"], "running", "{frozen}");
    assert_eq!(
        frozen["frozen"]["reason"], "ended without a result",
        "{frozen}"
    );
    assert_eq!(frozen["lost_continue"]["failed"], true, "{frozen}");

    let retry = ok(
        run(
            later,
            &[
                "queue",
                "claim",
                &path,
                "--item",
                "task two",
                "--owner",
                "subagent:two-retry",
            ],
        ),
        "claim fresh retry",
    );
    assert!(retry.contains("generation 2"), "{retry}");
    ok(
        run(
            later,
            &[
                "queue",
                "start",
                &path,
                "--item",
                "task two",
                "--owner",
                "subagent:two-retry",
                "--generation",
                "2",
                "--worker-id",
                "worker:two-retry",
            ],
        ),
        "start fresh retry",
    );
    let claims = run(later, &["queue", "claims", &path]);
    assert!(claims.status.success());
    let claims: serde_json::Value = serde_json::from_slice(&claims.stdout).unwrap();
    let replacement = claims
        .as_array()
        .unwrap()
        .iter()
        .find(|claim| claim["item_text"] == "task two")
        .unwrap();
    assert_eq!(replacement["generation"], 2, "{replacement}");
    assert_eq!(replacement["owner"], "subagent:two-retry", "{replacement}");
    assert_eq!(replacement["state"], "running", "{replacement}");
}

/// `#workerresponsestructure` (agent-doc-bugs.md, 2026-11-05): the
/// decisionidstamp hand-back reached `agent:exchange` as one quoted paragraph,
/// so its open item (">512-commit documents can miss answers") was buried and
/// never became queue work. A labelled hand-back now renders into the
/// response structure, and each open item lands as a follow-up row right after
/// the originating head in the same successor; a repeated report adds no
/// duplicate rows, and the session still admits the next turn.
#[test]
fn report_complete_structures_the_handback_and_queues_its_action_items() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_run_session(dir.path(), &["- do [#alpha]", "- later work"]);
    let path = doc.to_str().unwrap();
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
            ],
        ),
        "claim",
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "start",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
                "--generation",
                "1",
                "--worker-id",
                "worker:alpha",
            ],
        ),
        "start",
    );
    let handback = concat!(
        "**Summary:** fixed the alpha defect.\n",
        "**Verification:** make check exit=0; nextest 12 passed, 0 failed\n",
        "**Action items:**\n",
        "- >512-commit documents can miss answers\n",
        "- the gutter does not show the stamp yet\n",
        "**Next actions:** none",
    );
    let report = |what: &str| {
        let output = run_agent_doc(
            dir.path(),
            &[
                "queue",
                "report-complete",
                "--prose-only",
                path,
                "--item",
                "do [#alpha]",
                "--generation",
                "1",
                "--response",
                handback,
            ],
        );
        assert_ok(&output, what);
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let stderr = report("report-complete");
    assert!(
        stderr.contains("Queued 2 action item(s) as follow-up row(s)")
            && stderr.contains("queue id `default`"),
        "{stderr}"
    );

    let written = fs::read_to_string(&doc).unwrap();
    assert!(
        written.contains(concat!(
            "> **Subagent response:**\n>\n",
            "> **Summary**\n>\n> fixed the alpha defect.\n>\n",
            "> **Verification**\n>\n> make check exit=0; nextest 12 passed, 0 failed\n>\n",
            "> **Action items**\n>\n",
            "> - >512-commit documents can miss answers\n",
            "> - the gutter does not show the stamp yet\n>\n",
            "> **Next actions**\n>\n> none\n",
        )),
        "the hand-back renders into the response structure: {written}"
    );
    // Rows may carry a lifecycle marker (`📥`) once a later pass projects them.
    let first = "follow up on an action item from `alpha`: >512-commit documents can miss answers";
    let second = "follow up on an action item from `alpha`: the gutter does not show the stamp yet";
    let rows = run_rows(&doc);
    let position = |needle: &str| {
        rows.iter()
            .position(|row| row.ends_with(needle))
            .unwrap_or_else(|| panic!("{needle:?} missing: {rows:#?}"))
    };
    let alpha = rows
        .iter()
        .position(|row| row.ends_with("do [#alpha]"))
        .unwrap_or_else(|| panic!("origin row missing: {rows:#?}"));
    assert_eq!(position(first), alpha + 1, "{rows:#?}");
    assert_eq!(position(second), alpha + 2, "{rows:#?}");
    assert!(position("later work") > alpha + 2, "{rows:#?}");

    let stderr = report("repeated report-complete");
    assert!(
        stderr.contains("2 action item(s) already have a follow-up row"),
        "{stderr}"
    );
    let rows = run_rows(&doc);
    assert_eq!(
        rows.iter().filter(|row| row.ends_with(first)).count(),
        1,
        "{rows:#?}"
    );
    assert_eq!(
        rows.iter().filter(|row| row.ends_with(second)).count(),
        1,
        "{rows:#?}"
    );

    assert_session_check_does_not_accuse_patchback(dir.path(), &doc);
    assert_preflight_admits(dir.path(), &doc, 1);
}

/// `#handbackopenitemstriage` (agent-doc-bugs.md, 2026-11-06): the
/// #recommendationinrow hand-back listed six notes under Open items, and each
/// became a row offered for dispatch in a `go` queue. This is that hand-back's
/// shape: nothing is queued, every bullet stays in the cell, and the report
/// line asks the coordinator to triage them.
#[test]
fn report_complete_holds_open_items_for_triage_instead_of_queueing_them() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_run_session(dir.path(), &["- do [#alpha]", "- later work"]);
    let path = doc.to_str().unwrap();
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
            ],
        ),
        "claim",
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "start",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
                "--generation",
                "1",
                "--worker-id",
                "worker:alpha",
            ],
        ),
        "start",
    );
    let before = run_rows(&doc);
    let handback = concat!(
        "**Summary:** recommended choices show in the row.\n",
        "**Verification:** make check exit=0\n",
        "**Open items:**\n",
        "- **Cleanup.** `cargo clean` is done.\n",
        "- **Live document.** Nothing was applied to it.\n",
        "**Next actions:** none",
    );
    let output = run_agent_doc(
        dir.path(),
        &[
            "queue",
            "report-complete",
            "--prose-only",
            path,
            "--item",
            "do [#alpha]",
            "--generation",
            "1",
            "--response",
            handback,
        ],
    );
    assert_ok(&output, "report-complete");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("2 open item(s) were held for triage and NOT queued"),
        "{stderr}"
    );
    assert!(stderr.contains("queue follow-up"), "{stderr}");

    let rows = run_rows(&doc);
    assert_eq!(rows.len(), before.len(), "{rows:#?}");
    assert!(
        !rows.iter().any(|row| row.contains("follow up on")),
        "{rows:#?}"
    );
    let written = fs::read_to_string(&doc).unwrap();
    assert!(
        written.contains(concat!(
            "> **Open items**\n>\n",
            "> - **Cleanup.** `cargo clean` is done.\n",
            "> - **Live document.** Nothing was applied to it.\n",
        )),
        "{written}"
    );
}

/// `#handbackrefusedcommandexact`: two identical heads refuse placement, and
/// the report line's command is the real origin plus each unplaced row. After
/// the duplicate is removed, that printed line is what queues them.
#[test]
fn report_complete_prints_a_runnable_command_when_placement_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_run_session(
        dir.path(),
        &["- do [#alpha]", "- do [#alpha]", "- later work"],
    );
    let path = doc.to_str().unwrap();
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
            ],
        ),
        "claim",
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "start",
                path,
                "--item",
                "do [#alpha]",
                "--owner",
                "subagent:alpha",
                "--generation",
                "1",
                "--worker-id",
                "worker:alpha",
            ],
        ),
        "start",
    );
    let handback = concat!(
        "**Summary:** fixed the alpha defect.\n",
        "**Action items:**\n",
        "- >512-commit documents can miss answers\n",
        "- the gutter does not show the stamp yet\n",
        "**Next actions:** none",
    );
    let output = run_agent_doc(
        dir.path(),
        &[
            "queue",
            "report-complete",
            "--prose-only",
            path,
            "--item",
            "do [#alpha]",
            "--generation",
            "1",
            "--response",
            handback,
        ],
    );
    assert_ok(&output, "report-complete");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let command = stderr
        .lines()
        .find(|line| line.starts_with("agent-doc queue follow-up"))
        .unwrap_or_else(|| panic!("no runnable command:\n{stderr}"))
        .to_string();
    assert!(
        !command.contains("<origin>") && !command.contains("<text>"),
        "{command}"
    );
    assert!(command.contains("--id default"), "{command}");
    assert!(command.contains("--after "), "{command}");
    assert!(command.contains("do [#alpha]"), "{command}");
    assert!(command.contains("--item 'follow up on an action item from `alpha`: >512-commit documents can miss answers'"), "{command}");
    assert!(command.contains("--item 'follow up on an action item from `alpha`: the gutter does not show the stamp yet'"), "{command}");
    assert!(command.contains(path), "{command}");

    let written = fs::read_to_string(&doc).unwrap();
    let disambiguated = drop_second_run_row(&written, "do [#alpha]");
    fs::write(&doc, &disambiguated).unwrap();
    let rerun = run_printed_command(dir.path(), &command);
    assert_ok(&rerun, "printed queue follow-up");
    let rows = run_rows(&doc);
    let first = "follow up on an action item from `alpha`: >512-commit documents can miss answers";
    let second = "follow up on an action item from `alpha`: the gutter does not show the stamp yet";
    let position = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} missing: {rows:#?}"))
    };
    let alpha = rows
        .iter()
        .position(|row| row.contains("do [#alpha]") && !row.contains("follow up"))
        .unwrap_or_else(|| panic!("origin row missing: {rows:#?}"));
    assert_eq!(
        rows.iter()
            .filter(|row| row.contains("do [#alpha]") && !row.contains("follow up"))
            .count(),
        1,
        "{rows:#?}"
    );
    assert!(
        position(first) > alpha && position(second) > position(first),
        "{rows:#?}"
    );
}

fn drop_second_run_row(content: &str, origin: &str) -> String {
    let start = content.find("<!-- agent:run ").expect("run");
    let body_start = start + content[start..].find('\n').unwrap() + 1;
    let end = content.find("<!-- /agent:run -->").expect("run close");
    let mut seen = 0;
    let mut body = String::new();
    for line in content[body_start..end].split_inclusive('\n') {
        let work = line.trim_end_matches(['\n', '\r']).trim_start();
        let is_origin = work
            .strip_prefix("- ")
            .is_some_and(|rest| !rest.contains("follow up") && rest.contains(origin));
        if is_origin {
            seen += 1;
            if seen == 2 {
                continue;
            }
        }
        body.push_str(line);
    }
    assert_eq!(
        seen,
        2,
        "expected two origin rows:\n{}",
        &content[body_start..end]
    );
    let mut out = String::new();
    out.push_str(&content[..body_start]);
    out.push_str(&body);
    out.push_str(&content[end..]);
    out
}

fn run_printed_command(dir: &std::path::Path, command: &str) -> std::process::Output {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_agent-doc"));
    let bin_dir = binary.parent().expect("binary dir");
    let path = std::env::var("PATH").unwrap_or_default();
    Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .env("PATH", format!("{}:{path}", bin_dir.display()))
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .output()
        .unwrap()
}

/// `#handbackopenitemstriage`: a hand-back from a nested row put its follow-ups
/// at the top level between the parent's children. The next child then read as
/// a child of the last follow-up, which made that row a parent rather than an
/// executable leaf, so `queue consume --ack-text` could not reach it. Rows now
/// follow the whole top-level row: every child keeps its parent, and each
/// follow-up stays acknowledgeable.
#[test]
fn followups_of_a_nested_origin_leave_the_parent_children_alone_and_stay_acknowledgeable() {
    let dir = tempfile::tempdir().unwrap();
    let doc = committed_run_session(
        dir.path(),
        &[
            "- parent work",
            "  - leaf work",
            "  - sibling work",
            "- later work",
        ],
    );
    let path = doc.to_str().unwrap();
    let item = "parent work: leaf work";
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "claim",
                path,
                "--item",
                item,
                "--owner",
                "subagent:leaf",
            ],
        ),
        "claim",
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "start",
                path,
                "--item",
                item,
                "--owner",
                "subagent:leaf",
                "--generation",
                "1",
                "--worker-id",
                "worker:leaf",
            ],
        ),
        "start",
    );
    let handback = concat!(
        "**Summary:** done\n",
        "**Action items:**\n",
        "- wire the gutter stamp\n",
        "**Next actions:** none",
    );
    assert_ok(
        &run_agent_doc(
            dir.path(),
            &[
                "queue",
                "report-complete",
                "--prose-only",
                path,
                "--item",
                item,
                "--generation",
                "1",
                "--response",
                handback,
            ],
        ),
        "report-complete",
    );

    // The quote is the leaf's work text. A lifecycle marker on the origin
    // (`☑️ [#subagent:leaf#1]`) is not part of it (`#followuporiginlabelmarker`).
    let followup = "follow up on an action item from queue item “leaf work”: wire the gutter stamp";
    let rows = run_rows(&doc);
    let position = |needle: &str| {
        rows.iter()
            .position(|row| row.ends_with(needle))
            .unwrap_or_else(|| panic!("{needle:?} missing: {rows:#?}"))
    };
    assert!(
        rows[position("sibling work")].starts_with("  - "),
        "{rows:#?}"
    );
    assert_eq!(
        position("sibling work") + 1,
        position(followup),
        "{rows:#?}"
    );
    assert!(rows[position(followup)].starts_with("- "), "{rows:#?}");
    assert_eq!(position(followup) + 1, position("later work"), "{rows:#?}");

    let output = run_agent_doc(
        dir.path(),
        &["queue", "consume", path, "--ack-text", followup],
    );
    assert_ok(&output, "ack the follow-up row");
    let rows = run_rows(&doc);
    assert!(
        rows.iter()
            .any(|row| row.contains("sibling work") && !row.contains("~~")),
        "the parent's other child is untouched: {rows:#?}"
    );
}

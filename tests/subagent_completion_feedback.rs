use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI, and
/// a controller launch by that CLI is production behavior, so every spawned
/// command opts in with this reason.
const CONTROLLER_LAUNCH_REASON: &str = "queue claim/repair CLI commands reach the project controller, which they launch as production does";

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

    let started = Instant::now();
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
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "four one-shot reporters exceeded the bounded cross-process owner budget"
    );

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
    assert_eq!(fs::read_to_string(&doc).unwrap(), original, "dry run writes nothing");

    let (stdout, _) = run(&["--apply"]);
    assert!(stdout.contains("retired completion cell key=stalekey"), "{stdout}");
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
            "### Re: dispatch — gpt-5 · 2026-10-14T22:00-04:00\n\nDispatched four subagents.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "{}",
            "<!-- /agent:queue -->\n",
        ),
        queue,
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
    let output = run_agent_doc(dir, &["preflight", doc.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "preflight must admit a document carrying binary completion feedback: stderr={stderr}"
    );
    assert!(!stderr.contains("failed closed"), "stderr={stderr}");
    assert!(!stderr.contains("refused to replay"), "stderr={stderr}");
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
    let output = run_agent_doc(dir, &["preflight", doc.to_str().unwrap()]);
    assert_ok(&output, "preflight");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "preflight stdout is not JSON ({err}): {}",
            String::from_utf8_lossy(&output.stdout)
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
                let expected = format!("- {marker} [#subagent:{id}#1] do [#{id}]");
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
        assert_ok(
            &run_agent_doc(
                dir.path(),
                &[
                    "queue",
                    "report-complete",
                    path,
                    "--item",
                    &format!("do [#{id}]"),
                    "--generation",
                    "1",
                    "--response",
                    &format!("Implemented and tested the {id} fix; branch fix/{id} is pushed."),
                ],
            ),
            "queue report-complete",
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

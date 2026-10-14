use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
        let status = Command::new(binary)
            .current_dir(dir.path())
            .args([
                "queue",
                "claim",
                doc.to_str().unwrap(),
                "--item",
                item,
                "--owner",
                &format!("subagent:{}", item.replace(' ', "-")),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "claim failed for {item}: {status}");
    }

    let started = Instant::now();
    let mut children = items
        .into_iter()
        .map(|item| {
            Command::new(binary)
                .current_dir(dir.path())
                .args([
                    "queue",
                    "report-complete",
                    doc.to_str().unwrap(),
                    "--item",
                    item,
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
            2,
            "queue + notice for {item}"
        );
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
        .current_dir(dir.path())
        .args(["queue", "repair-completion-feedback", doc.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("0 artifacts repaired"));
    assert_eq!(fs::read_to_string(&doc).unwrap(), repaired);
}

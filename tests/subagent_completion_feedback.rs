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
}

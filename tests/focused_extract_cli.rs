use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::process::Command;

fn source_document() -> &'static str {
    r#"---
agent_doc_session: 00000000-0000-0000-0000-000000000001
agent: codex
agent_doc_format: template
agent_doc_write: crdt
---

# Mixed SDK

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
❯ Decide the legal policy [#legal1]
### Re: Legal

Counsel requires an age gate for #legal1.
### Re: Development

The client build is unrelated.
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
- [#legal1]: Draft the policy
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#legal1] Implement the age gate
- [ ] [#dev1] Build the client
<!-- /agent:backlog -->

## Todo

<!-- agent:todo -->
- [ ] [#legal1] Ask counsel
<!-- /agent:todo -->

## Review

<!-- agent:review -->
- [/] [#legal1] Counsel approval
<!-- /agent:review -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Notes

<!-- agent:notes -->
<!-- /agent:notes -->
"#
}

#[test]
fn agent_selection_previews_applies_and_reruns_idempotently() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("sdk.md");
    let target = dir.path().join("sdk").join("sdk-legal.md");
    let selection = dir.path().join("legal-selection.json");
    fs::write(&source, source_document()).unwrap();
    fs::write(
        &selection,
        r#"{
  "focus": "SDK legal and compliance",
  "rationale": "Keep counsel decisions and tracked legal work together.",
  "ids": ["legal1"]
}"#,
    )
    .unwrap();

    Command::new("git")
        .current_dir(dir.path())
        .args(["init", "--quiet"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(dir.path())
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(dir.path())
        .args(["config", "user.name", "Test User"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(dir.path())
        .args(["add", "sdk.md"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(dir.path())
        .args(["commit", "--quiet", "-m", "fixture", "--no-verify"])
        .status()
        .unwrap();

    cargo_bin_cmd!("agent-doc")
        .current_dir(dir.path())
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .args([
            "extract",
            source.to_str().unwrap(),
            target.to_str().unwrap(),
            "--selection",
            selection.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("SDK legal and compliance"))
        .stdout(predicate::str::contains("\"component\": \"backlog\""));
    assert!(!target.exists(), "preview must not create the destination");

    for expected_already_applied in [false, true] {
        cargo_bin_cmd!("agent-doc")
            .current_dir(dir.path())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .args([
                "extract",
                source.to_str().unwrap(),
                target.to_str().unwrap(),
                "--selection",
                selection.to_str().unwrap(),
                "--apply",
                "--bypass-claim",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(format!(
                "\"already_applied\": {expected_already_applied}"
            )));
    }

    let source_after = fs::read_to_string(&source).unwrap();
    let target_after = fs::read_to_string(&target).unwrap();
    assert!(!source_after.contains("Ask counsel"));
    assert!(source_after.contains("Build the client"));
    assert!(target_after.contains("- [ ] [#legal1] Ask counsel"));
    assert!(target_after.contains("- [ ] [#legal1] Implement the age gate"));
    assert!(target_after.contains("- [/] [#legal1] Counsel approval"));
    assert!(target_after.contains("Counsel requires an age gate"));
    assert!(target_after.contains("agent-doc-extract"));
}

#[test]
fn legacy_extract_rejects_plain_markdown_target_without_mutating_source() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("sdk-dev.md");
    let target = dir.path().join("sdk-demo.md");
    let source_before = source_document();
    let target_before = "# SDK demo plan\n\nA regular plan, not an agent document.\n";
    fs::write(&source, source_before).unwrap();
    fs::write(&target, target_before).unwrap();

    cargo_bin_cmd!("agent-doc")
        .current_dir(dir.path())
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .args([
            "extract",
            source.to_str().unwrap(),
            target.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("legacy extract target"))
        .stderr(predicate::str::contains("is not an agent document"));

    assert_eq!(fs::read_to_string(&source).unwrap(), source_before);
    assert_eq!(fs::read_to_string(&target).unwrap(), target_before);
}

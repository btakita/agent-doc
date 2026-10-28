use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::process::Command;
use tmux_router::{Registry as SessionRegistry, RegistryEntry as SessionEntry};

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
        .stdout(predicate::str::contains("\"component\": \"backlog\""))
        .stderr(predicate::str::contains(format!(
            "preview only; no documents were changed and destination {} was not created",
            target.display()
        )));
    assert!(!target.exists(), "preview must not create the destination");

    for expected_already_applied in [false, true] {
        cargo_bin_cmd!("agent-doc")
            .current_dir(dir.path())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env(
                "AGENT_DOC_TEST_CONTROLLER_LAUNCH_REASON",
                "focused extract live-editor authority check",
            )
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

/// `#extractlayout`: the extracted game-sessions.md put every heading on its
/// own line above the opener and headed `agent:run` `## Queue`. A new target's
/// headings share their opener's line, `agent:run` is `## Run`, and a heading
/// the source wrote on an opener line is kept verbatim.
#[test]
fn new_target_heads_each_component_on_its_opener_line() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("sdk.md");
    let target = dir.path().join("sdk").join("sdk-legal.md");
    let heading_anchored = source_document()
        .replacen(
            "## Todo\n\n<!-- agent:todo -->",
            "## Todo <!-- agent:todo -->",
            1,
        )
        .replacen(
            "## Icebox\n\n<!-- agent:icebox -->",
            "## Parking Lot <!-- agent:icebox -->",
            1,
        );
    fs::write(&source, heading_anchored).unwrap();
    git_fixture(dir.path(), "sdk.md");

    apply_legal_extract(dir.path(), &source, &target).success();

    let target_after = fs::read_to_string(&target).unwrap();
    for expected in [
        "\n## Status <!-- agent:status patch=replace -->\n",
        "\n## Exchange <!-- agent:exchange patch=append -->\n",
        "\n## Run <!-- agent:run priority subagents clear -->\n",
        "\n## Todo <!-- agent:todo priority -->\n",
        "\n## Backlog <!-- agent:backlog priority queue -->\n",
        "\n## Parking Lot <!-- agent:icebox -->\n",
        "\n## Notes <!-- agent:notes -->\n",
    ] {
        assert!(
            target_after.contains(expected),
            "missing {expected:?}:\n{target_after}"
        );
    }
    assert!(
        !target_after.contains("## Queue <!-- agent:run"),
        "{target_after}"
    );
    let standalone_openers: Vec<&str> = target_after
        .lines()
        .filter(|line| line.starts_with("<!-- agent:") && !line.starts_with("<!-- agent:boundary"))
        .collect();
    assert!(
        standalone_openers.is_empty(),
        "every generated opener shares a heading line: {standalone_openers:?}\n{target_after}"
    );
    assert!(
        !target_after
            .lines()
            .any(|line| line.starts_with("## ") && !line.contains("<!-- agent:")),
        "no generated heading stands alone above its opener:\n{target_after}"
    );
}

#[test]
fn new_target_inherits_the_source_effective_harness_when_agent_is_absent() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("legacy-claude.md");
    let target = dir.path().join("legacy-claude-legal.md");
    let source_without_agent = source_document().replacen(
        "agent: codex\n",
        "resume:\n  claude: claude-session-id\n",
        1,
    );
    fs::write(&source, source_without_agent).unwrap();
    git_fixture(dir.path(), "legacy-claude.md");

    apply_legal_extract(dir.path(), &source, &target).success();

    let target_after = fs::read_to_string(&target).unwrap();
    assert!(target_after.contains("agent: claude\n"), "{target_after}");
    assert!(!target_after.contains("agent: codex\n"), "{target_after}");
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

#[test]
fn focused_extract_apply_preserves_the_source_claim_without_claiming_the_target() {
    let dir = tempfile::TempDir::new().unwrap();
    let source = dir.path().join("sdk-demo.md");
    let target = dir.path().join("sdk").join("bossroom-ui-integration.md");
    let selection = dir.path().join("ui-selection.json");
    fs::write(&source, source_document()).unwrap();
    fs::write(
        &selection,
        r#"{
  "focus": "Boss Room UI integration",
  "rationale": "Keep the extracted child routable without stealing the parent pane.",
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
        .args(["add", "sdk-demo.md"])
        .status()
        .unwrap();
    Command::new("git")
        .current_dir(dir.path())
        .args(["commit", "--quiet", "-m", "fixture", "--no-verify"])
        .status()
        .unwrap();

    let source_path = source.display().to_string();
    let mut registry = SessionRegistry::new();
    registry.insert(
        "00000000-0000-0000-0000-000000000001".to_string(),
        SessionEntry {
            pane: "%42".to_string(),
            pid: std::process::id(),
            cwd: dir.path().display().to_string(),
            started: "2026-10-19T00:00:00Z".to_string(),
            session_id: "00000000-0000-0000-0000-000000000001".to_string(),
            file: source_path.clone(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    agent_doc_session_registry_io::save_in(dir.path(), &registry).unwrap();

    cargo_bin_cmd!("agent-doc")
        .current_dir(dir.path())
        .env_remove("TMUX")
        .env("TMUX_PANE", "%42")
        .env(
            "AGENT_DOC_TEST_CONTROLLER_LAUNCH_REASON",
            "focused extract claim boundary",
        )
        .args([
            "extract",
            source.to_str().unwrap(),
            target.to_str().unwrap(),
            "--selection",
            selection.to_str().unwrap(),
            "--apply",
        ])
        .assert()
        .success();

    let registry_after = agent_doc_session_registry_io::load_in(dir.path()).unwrap();
    assert_eq!(registry_after.len(), 1);
    let source_claim = registry_after.values().next().unwrap();
    assert_eq!(source_claim.pane, "%42");
    assert_eq!(source_claim.file, source_path);
    assert!(
        registry_after
            .values()
            .all(|entry| entry.file != target.display().to_string()),
        "extract must not auto-claim its destination"
    );
}

fn git_fixture(dir: &std::path::Path, tracked: &str) {
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test User"],
        vec!["add", tracked],
        vec!["commit", "--quiet", "-m", "fixture", "--no-verify"],
    ] {
        assert!(
            Command::new("git")
                .current_dir(dir)
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
}

fn apply_legal_extract(
    dir: &std::path::Path,
    source: &std::path::Path,
    target: &std::path::Path,
) -> assert_cmd::assert::Assert {
    cargo_bin_cmd!("agent-doc")
        .current_dir(dir)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env(
            "AGENT_DOC_TEST_CONTROLLER_LAUNCH_REASON",
            "focused extract target-loss check",
        )
        .args([
            "extract",
            source.to_str().unwrap(),
            target.to_str().unwrap(),
            "--ids",
            "legal1",
            "--apply",
            "--bypass-claim",
        ])
        .assert()
}

/// GH #324: the destination kept the operation marker but lost the moved
/// exchange turns, and the source still held them (same bytes, so the same
/// operation ID). Re-running the extract must not strip the source's only
/// remaining copy; it refuses, names the lost node IDs, and logs the loss.
/// The first apply also leaves a recovery copy keyed by operation ID.
#[test]
fn rerun_against_a_marked_target_that_lost_exchange_turns_refuses_and_keeps_the_source() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let source = dir.path().join("sdk.md");
    let target = dir.path().join("sdk-legal.md");
    fs::write(&source, source_document()).unwrap();
    git_fixture(dir.path(), "sdk.md");

    let applied = apply_legal_extract(dir.path(), &source, &target).success();
    let preview: serde_json::Value = serde_json::from_slice(&applied.get_output().stdout).unwrap();
    let operation_id = preview["operation_id"].as_str().unwrap().to_string();

    let recovery = dir
        .path()
        .join(".agent-doc/archives/extract")
        .join(format!("{operation_id}.md"));
    let recovery_text = fs::read_to_string(&recovery).expect("recovery copy written on apply");
    assert!(recovery_text.contains("Counsel requires an age gate for #legal1."));
    assert!(recovery_text.contains("Ask counsel"));
    assert!(!recovery_text.contains("The client build is unrelated."));

    // Reproduce the incident state: the source still carries the turns, the
    // target carries the marker and the list items but not the turns.
    fs::write(&source, source_document()).unwrap();
    let lost_turns = "❯ Decide the legal policy [#legal1]\n### Re: Legal\n\nCounsel requires an age gate for #legal1.\n";
    let target_applied = fs::read_to_string(&target).unwrap();
    assert!(target_applied.contains(lost_turns));
    assert!(target_applied.contains(&format!("operation=\"{operation_id}\"")));
    let target_lost = target_applied.replacen(lost_turns, "", 1);
    fs::write(&target, &target_lost).unwrap();

    apply_legal_extract(dir.path(), &source, &target)
        .failure()
        .stderr(predicate::str::contains(format!(
            "focused-extract operation {operation_id} is already marked"
        )))
        .stderr(predicate::str::contains("exchange:r:"))
        .stderr(predicate::str::contains("exchange:p:"))
        .stderr(predicate::str::contains("no documents were changed"));

    assert_eq!(fs::read_to_string(&source).unwrap(), source_document());
    assert_eq!(fs::read_to_string(&target).unwrap(), target_lost);
    let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains(&format!(
            "focused_extract_target_lost_nodes operation={operation_id} "
        )),
        "{ops_log}"
    );
}

/// GH #324 gap: the extract already stripped the source, then the destination
/// lost the moved turns while its `[#legal1]` list items survived. The re-run
/// derives a new operation ID and moves nothing, and selector coverage alone
/// reported `already_applied`. It now verifies the target against the first
/// operation's recovery copy and refuses, naming the lost node IDs and the
/// recovery copy, with both documents unchanged and the loss logged.
#[test]
fn rerun_after_a_stripped_source_refuses_when_the_target_lost_recorded_turns() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let source = dir.path().join("sdk.md");
    let target = dir.path().join("sdk-legal.md");
    fs::write(&source, source_document()).unwrap();
    git_fixture(dir.path(), "sdk.md");

    let applied = apply_legal_extract(dir.path(), &source, &target).success();
    let preview: serde_json::Value = serde_json::from_slice(&applied.get_output().stdout).unwrap();
    let operation_id = preview["operation_id"].as_str().unwrap().to_string();
    let recovery = dir
        .path()
        .join(".agent-doc/archives/extract")
        .join(format!("{operation_id}.md"));
    assert!(recovery.exists(), "recovery copy written on apply");

    let source_stripped = fs::read_to_string(&source).unwrap();
    assert!(!source_stripped.contains("Counsel requires an age gate"));
    let lost_turns = "❯ Decide the legal policy [#legal1]\n### Re: Legal\n\nCounsel requires an age gate for #legal1.\n";
    let target_applied = fs::read_to_string(&target).unwrap();
    assert!(target_applied.contains(lost_turns));
    let target_lost = target_applied.replacen(lost_turns, "", 1);
    assert!(
        target_lost.contains("- [ ] [#legal1] Ask counsel"),
        "surviving list items still mention the selector"
    );
    fs::write(&target, &target_lost).unwrap();

    let refused = apply_legal_extract(dir.path(), &source, &target)
        .failure()
        .stdout(predicate::str::contains("\"already_applied\": true").not())
        .stderr(predicate::str::contains(format!(
            "focused-extract operation {operation_id} already removed this selection"
        )))
        .stderr(predicate::str::contains("exchange:r:"))
        .stderr(predicate::str::contains("exchange:p:"))
        .stderr(predicate::str::contains("no documents were changed"));
    let stderr = String::from_utf8_lossy(&refused.get_output().stderr).to_string();
    assert!(
        stderr.contains(&format!(".agent-doc/archives/extract/{operation_id}.md")),
        "{stderr}"
    );

    assert_eq!(fs::read_to_string(&source).unwrap(), source_stripped);
    assert_eq!(fs::read_to_string(&target).unwrap(), target_lost);
    let ops_log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains(&format!(
            "focused_extract_target_lost_nodes operation={operation_id} "
        )) && ops_log.contains(&format!(
            "recovery_copy={}",
            recovery.canonicalize().unwrap().display()
        )),
        "{ops_log}"
    );
}

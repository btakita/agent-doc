//! `agent-doc queue brief` (`#waypostauthorization`): the operator
//! authorization a coordinator pastes into a dispatched queue subagent's prompt.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn document(frontmatter: &str, queue_attr: &str, queue: &[&str], backlog: &[&str]) -> String {
    let queue: String = queue.iter().map(|p| format!("- {p}\n")).collect();
    let backlog: String = backlog.iter().map(|b| format!("- {b}\n")).collect();
    format!(
        "---\nagent_doc_session: brief\nagent_doc_format: template\n{frontmatter}---\n\n\
         ## Backlog\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n\n\
         ## Queue\n\n<!-- agent:queue {queue_attr} go -->\n{queue}<!-- /agent:queue -->\n"
    )
}

const PRESETS: &str = "prompt_presets:\n  \"#spec-test-build-install-commit-push\": \"update spec + tests. build + install for local testing. commit + push\"\n";
const PRESET_BODY: &str = "update spec + tests. build + install for local testing. commit + push";

fn write_doc(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, content).unwrap();
    (dir, doc)
}

#[test]
fn brief_prints_preset_and_backlog_authorization_for_an_id_head() {
    let (_dir, doc) = write_doc(&document(
        PRESETS,
        "subagents preset=\"#spec-test-build-install-commit-push\"",
        &["do [#waypost]", "rename foo to bar"],
        &["[ ] [#waypost] Post authorization to Claude Code subagents"],
    ));
    cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", "#waypost"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#waypostauthorization"))
        .stdout(predicate::str::contains("> do [#waypost]"))
        .stdout(predicate::str::contains(
            "> Post authorization to Claude Code subagents",
        ))
        .stdout(predicate::str::contains(format!("> {PRESET_BODY}")))
        .stdout(predicate::str::contains("Never run `make install`"))
        .stdout(predicate::str::contains("AUTHORIZATION NOT RESOLVED").not());
}

#[test]
fn brief_json_for_a_free_text_head_under_a_preset() {
    let (_dir, doc) = write_doc(&document(
        PRESETS,
        "subagents preset=\"#spec-test-build-install-commit-push\"",
        &["rename foo to bar"],
        &[],
    ));
    let output = cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", "rename foo to bar", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "resolved");
    assert_eq!(json["item"], "rename foo to bar");
    assert_eq!(
        json["presets"][0]["name"],
        "#spec-test-build-install-commit-push"
    );
    assert_eq!(json["presets"][0]["source"], "queue_attr");
    assert_eq!(json["presets"][0]["body"], PRESET_BODY);
    assert!(json.get("tracked_items").is_none());
    assert!(
        json["subagent_prompt_preamble"]
            .as_str()
            .unwrap()
            .contains(PRESET_BODY)
    );
}

#[test]
fn brief_without_a_queue_preset_is_item_text_only() {
    let (_dir, doc) = write_doc(&document("", "subagents", &["rename foo to bar"], &[]));
    let output = cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", "rename foo to bar", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "item_text_only");
    assert!(json.get("presets").is_none());
}

#[test]
fn brief_fails_closed_on_a_preset_missing_from_frontmatter() {
    let (_dir, doc) = write_doc(&document(
        "",
        "subagents preset=\"#ship-it\"",
        &["do [#abc]"],
        &["[ ] [#abc] thing"],
    ));
    let output = cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", "#abc", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "unresolved_preset");
    assert_eq!(json["presets"][0]["name"], "#ship-it");
    assert_eq!(json["presets"][0]["resolved"], false);
    assert!(json["presets"][0].get("body").is_none(), "{json}");
    let preamble = json["subagent_prompt_preamble"].as_str().unwrap();
    assert!(
        preamble.contains("AUTHORIZATION NOT RESOLVED"),
        "{preamble}"
    );
    assert!(preamble.contains("Do not commit, push"), "{preamble}");
}

#[test]
fn brief_refuses_an_item_that_is_not_a_live_head() {
    let (_dir, doc) = write_doc(&document("", "subagents", &["do [#abc]"], &[]));
    cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", "#nothere"])
        .assert()
        .failure();
}

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

const PRESETS: &str = "prompt_presets:\n  \"#spec-test-build-install-commit-push\": \"update spec + tests. build + install for local testing. commit + push\"\n  \"#auth\": \"I authorize you to do this.\"\n";
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
        &["do [#waypost] #auth", "rename foo to bar"],
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
        .stdout(predicate::str::contains(
            "Operator authorization, item preset `#auth`",
        ))
        .stdout(predicate::str::contains("> I authorize you to do this."))
        .stdout(predicate::str::contains(
            "without asking the operator to repeat those words",
        ))
        .stdout(predicate::str::contains(
            "does not itself grant a harness-native tool permission",
        ))
        .stdout(predicate::str::contains("treat prior authorization as the answer").not())
        .stdout(predicate::str::contains("Never run `make install`"))
        .stdout(predicate::str::contains(
            "\n[#subagent-operator-request-<unique-id>] subagent operator request\n- *queue_item:*\n  - ",
        ))
        .stdout(predicate::str::contains(
            "Use a different marker id for every request packet in the session document",
        ))
        .stdout(predicate::str::contains("[agent-doc] subagent operator request").not())
        .stdout(predicate::str::contains("(#waypostrequest)").not())
        .stdout(predicate::str::contains("\n- *kind:* question | permission\n"))
        .stdout(predicate::str::contains(
            "\n- *request:*\n  - <exact question or exact tool/action requiring permission>\n",
        ))
        .stdout(predicate::str::contains(
            "\n- *context:*\n  - <why it is needed>\n  - <current state>\n  - <target/scope>\n  - <risk or effect>\n",
        ))
        .stdout(predicate::str::contains(
            "\n- *choices:*\n  - **<label>**: <effect>\n",
        ))
        .stdout(predicate::str::contains("- *choices:*\n  - <label>: <effect>").not())
        .stdout(predicate::str::contains("- *context:* <").not())
        .stdout(predicate::str::contains(
            "A chat reply is context, not a tool permission grant",
        ))
        .stdout(predicate::str::contains(format!(
            "- {}\n",
            agent_doc_queue::subagent_brief::SUBAGENT_SCRATCH_ISOLATION_RULE
        )))
        .stdout(predicate::str::contains(format!(
            "- {}\n",
            agent_doc_queue::subagent_brief::SUBAGENT_TMUX_ISOLATION_RULE
        )))
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

/// Regression for the live `tasks/docs.md` spelling: the plural component
/// attribute used to be ignored, so Codex received `item_text_only` and asked
/// the operator for permission even though `#auth` was present.
#[test]
fn brief_resolves_plural_presets_attribute_as_operator_authorization() {
    let (_dir, doc) = write_doc(&document(
        PRESETS,
        "subagents presets=\"#auth\"",
        &["review and push the docs change"],
        &[],
    ));
    let output = cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", "review and push the docs change", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "resolved", "{json}");
    assert_eq!(json["presets"][0]["name"], "#auth", "{json}");
    assert_eq!(json["presets"][0]["source"], "queue_attr", "{json}");
    assert_eq!(
        json["presets"][0]["body"], "I authorize you to do this.",
        "{json}"
    );
    let preamble = json["subagent_prompt_preamble"].as_str().unwrap();
    assert!(preamble.contains("Operator authorization, queue preset `#auth`"));
    assert!(!preamble.contains("No queue preset applies"));
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

#[test]
fn brief_does_not_grant_a_preset_quoted_in_inline_code() {
    let item = "#gh-issue: do not run `#upgrade` here";
    let (_dir, doc) = write_doc(&document(
        concat!(
            "prompt_presets:\n",
            "  '#upgrade': Upgrade agent-doc.\n",
            "  '#gh-issue': File a GitHub issue.\n",
        ),
        "subagents",
        &[item],
        &[],
    ));
    let output = cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", item, "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["status"], "resolved", "{json}");
    assert_eq!(json["presets"].as_array().unwrap().len(), 1, "{json}");
    assert_eq!(json["presets"][0]["name"], "#gh-issue", "{json}");
    assert_eq!(json["presets"][0]["source"], "item", "{json}");
    let preamble = json["subagent_prompt_preamble"].as_str().unwrap();
    assert!(preamble.contains("Operator authorization, item preset `#gh-issue`"));
    assert!(!preamble.contains("Operator authorization, item preset `#upgrade`"));
    assert!(!preamble.contains("> Upgrade agent-doc."));
}

/// `#idlessqueueidentity`: the live session document after the operator
/// deleted `id=` from its one `agent:run` and one `agent:loop`. `queue brief`
/// used to fail with "no live queue head matches ...; live heads: (none)";
/// it now resolves the unclaimed free-text row and names its origin queue
/// `run`, the kind id `queue follow-up --id` accepts.
#[test]
fn brief_resolves_a_free_text_row_of_an_idless_run_beside_an_idless_loop() {
    let item = "Why have the ☑️ run items not completed?";
    let content = format!(
        "---\nagent_doc_session: brief\nagent_doc_format: template\nqueue: go\n{PRESETS}---\n\n\
## Run <!-- agent:run subagents preset=\"#spec-test-build-install-commit-push\" priority go -->\n\
- 🚧 [#subagent:noargparity#1] `/agent-doc` without arguments\n\
- {item}\n\
<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop interval=\"1h\" priority -->\n\
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues.\n\
<!-- /agent:loop -->\n\n\
## Backlog <!-- agent:backlog priority queue -->\n<!-- /agent:backlog -->\n"
    );
    let (_dir, doc) = write_doc(&content);
    let output = cargo_bin_cmd!("agent-doc")
        .args(["queue", "brief"])
        .arg(&doc)
        .args(["--item", item, "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["item"], item, "{json}");
    assert_eq!(json["queue_id"], "run", "{json}");
    assert_eq!(json["status"], "resolved", "{json}");
    let preamble = json["subagent_prompt_preamble"].as_str().unwrap();
    assert!(preamble.contains("Origin queue id (dispatch-time provenance):\n> run"));
    assert!(preamble.contains("--id run --after <origin>"));
}

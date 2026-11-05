//! `agent-doc principles <FILE> [--json]` and the principle checks in
//! `agent-doc lint` (`#firstprincipleslist`).

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

struct Fixture {
    _temp: tempfile::TempDir,
    config_home: std::path::PathBuf,
    document: std::path::PathBuf,
    document_text: String,
}

/// A global principles file and a document that overrides one global
/// principle with a mismatched `scope:`, adds its own principle, and writes a
/// malformed one.
///
/// The fixture has no `.agent-doc` project root on purpose: reading the
/// current document under a project root would cold-launch a controller from
/// the spawned CLI. The project layer goes through the same
/// `composed_rules_and_principles` that the preflight unit tests drive with
/// ancestor `.agent-doc/project.md` files.
fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(config_home.join("agent-doc")).unwrap();
    std::fs::write(
        config_home.join("agent-doc/principles.md"),
        concat!(
            "<!-- agent:rules -->\n",
            "- principle [#velocity]: When a release is blocked, velocity over polish.\n",
            "- principle [#no-reask]: Never re-ask an answered question.\n",
            "  rationale: It spends operator attention twice.\n",
            "<!-- /agent:rules -->\n",
        ),
    )
    .unwrap();
    let document = temp.path().join("sample.md");
    let document_text = concat!(
        "---\nagent_doc_session: sample\n---\n\n",
        "<!-- agent:rules -->\n",
        "- principle [#velocity]: In this document, correctness over velocity.\n",
        "  scope: global\n",
        "- principle [#orders-first]: Order data is never dropped.\n",
        "- principle: forgot the id\n",
        "- Run the focused test first.\n",
        "<!-- /agent:rules -->\n\n",
        "<!-- agent:exchange -->\n",
        "prompt\n",
        "<!-- /agent:exchange -->\n",
    )
    .to_string();
    std::fs::write(&document, &document_text).unwrap();
    Fixture {
        _temp: temp,
        config_home,
        document,
        document_text,
    }
}

#[test]
fn principles_json_lists_composed_principles_and_warnings() {
    let fixture = fixture();
    let output = cargo_bin_cmd!("agent-doc")
        .env("XDG_CONFIG_HOME", &fixture.config_home)
        .args(["principles", "--json"])
        .arg(&fixture.document)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    let principles: Vec<(&str, &str, &str, usize)> = json["principles"]
        .as_array()
        .unwrap_or_else(|| panic!("{json:#}"))
        .iter()
        .map(|principle| {
            (
                principle["id"].as_str().unwrap(),
                principle["scope"].as_str().unwrap(),
                principle["statement"].as_str().unwrap(),
                principle["overrides"].as_array().map_or(0, Vec::len),
            )
        })
        .collect();
    assert_eq!(
        principles,
        vec![
            (
                "velocity",
                "document",
                "In this document, correctness over velocity.",
                1
            ),
            (
                "no-reask",
                "global",
                "Never re-ask an answered question.",
                0
            ),
            (
                "orders-first",
                "document",
                "Order data is never dropped.",
                0
            ),
        ],
        "{json:#}"
    );
    assert_eq!(
        json["principles"][1]["rationale"],
        "It spends operator attention twice."
    );
    let codes: Vec<&str> = json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|warning| warning["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        vec!["principle_scope_mismatch", "principle_malformed"],
        "{json:#}"
    );
    assert_eq!(
        std::fs::read_to_string(&fixture.document).unwrap(),
        fixture.document_text,
        "the listing is read-only"
    );
}

#[test]
fn principles_text_lists_each_principle_then_warnings() {
    let fixture = fixture();
    cargo_bin_cmd!("agent-doc")
        .env("XDG_CONFIG_HOME", &fixture.config_home)
        .arg("principles")
        .arg(&fixture.document)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("3 principle(s) in scope for")
                .and(predicate::str::contains(
                    "[#velocity] document: In this document, correctness over velocity.",
                ))
                .and(predicate::str::contains(
                    "  rationale: It spends operator attention twice.",
                ))
                .and(predicate::str::contains("  overrides: "))
                .and(predicate::str::contains("2 warning(s):"))
                .and(predicate::str::contains("  principle_malformed: ")),
        );
}

#[test]
fn principles_without_any_says_where_to_write_one() {
    let temp = tempfile::tempdir().unwrap();
    let document = temp.path().join("plain.md");
    std::fs::write(
        &document,
        "---\nagent_doc_session: plain\n---\n\n<!-- agent:exchange -->\nprompt\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    cargo_bin_cmd!("agent-doc")
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .arg("principles")
        .arg(&document)
        .assert()
        .success()
        .stdout(predicate::str::contains("No principles in scope for"));
}

#[test]
fn lint_reports_principle_warnings_without_blocking() {
    let fixture = fixture();
    cargo_bin_cmd!("agent-doc")
        .env("XDG_CONFIG_HOME", &fixture.config_home)
        .arg("lint")
        .arg(&fixture.document)
        .assert()
        .success()
        .stdout(predicate::str::contains("No blocking lint findings"))
        .stderr(
            predicate::str::contains("2 principle warning(s)")
                .and(predicate::str::contains(
                    "sample.md: warning: principle [#velocity] declares scope global",
                ))
                .and(predicate::str::contains("[principle_scope_mismatch]"))
                .and(predicate::str::contains("[principle_malformed]")),
        );
    assert_eq!(
        std::fs::read_to_string(&fixture.document).unwrap(),
        fixture.document_text
    );
}

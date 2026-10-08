use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

const CLEAN_DOCUMENT: &str = concat!(
    "---\nagent_doc_session: sample\n---\n\n",
    "<!-- agent:exchange -->\n",
    "prompt\n",
    "<!-- /agent:exchange -->\n",
);

#[test]
fn lint_is_read_only_and_reports_a_clean_document() {
    let temp = tempfile::tempdir().unwrap();
    let document = temp.path().join("sample.md");
    std::fs::write(&document, CLEAN_DOCUMENT).unwrap();

    cargo_bin_cmd!("agent-doc")
        .arg("lint")
        .arg(&document)
        .assert()
        .success()
        .stdout(predicate::str::contains("No blocking lint findings"));

    assert_eq!(std::fs::read_to_string(document).unwrap(), CLEAN_DOCUMENT);
}

#[test]
fn lint_prints_the_same_blocking_finding_as_closeout() {
    let temp = tempfile::tempdir().unwrap();
    let document = temp.path().join("malformed.md");
    let malformed = concat!(
        "---\nagent_doc_session: sample\n---\n\n",
        "<!-- agent:exchange -->\n",
        "prompt\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog mystery -->\n",
        "- [ ] work\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&document, malformed).unwrap();

    cargo_bin_cmd!("agent-doc")
        .arg("lint")
        .arg(&document)
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("agent-doc/malformed-attr")
                .and(predicate::str::contains("malformed.md:")),
        );

    assert_eq!(std::fs::read_to_string(document).unwrap(), malformed);
}

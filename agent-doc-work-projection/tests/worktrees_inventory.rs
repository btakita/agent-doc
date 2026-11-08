//! `agent:worktrees` is the managed-list inventory before `agent:recap`.

use agent_doc_work_projection::worktrees::{
    WorktreeCandidate, WorktreeRecord, active_records, reconcile,
};

fn record(topic: &str, branch: &str, directory: &str) -> WorktreeRecord {
    WorktreeRecord {
        topic: topic.to_string(),
        branch: branch.to_string(),
        directory: directory.to_string(),
    }
}

const DOC: &str = concat!(
    "## Status <!-- agent:status patch=replace -->\n",
    "<!-- /agent:status -->\n",
    "\n## Recap <!-- agent:recap patch=replace -->\n",
    "<!-- /agent:recap -->\n",
    "\n## Active <!-- agent:active priority -->\n",
    "<!-- /agent:active -->\n",
    "\n## Run <!-- agent:run -->\n",
    "<!-- /agent:run -->\n",
);

#[test]
fn inventory_lands_before_recap_and_is_a_fixed_point() {
    let records = vec![
        record("Ship the list", "feat/worktrees", "/tmp/wt-b"),
        record("Earlier topic", "feat/earlier", "/tmp/wt-a"),
    ];
    let updated = reconcile(DOC, &records)
        .unwrap()
        .expect("a live worktree inserts the section");
    let status = updated.find("<!-- /agent:status -->").unwrap();
    let worktrees = updated.find("<!-- agent:worktrees -->").unwrap();
    let recap = updated.find("<!-- agent:recap patch=replace -->").unwrap();
    let active = updated.find("<!-- agent:active priority -->").unwrap();
    assert!(
        status < worktrees && worktrees < recap && recap < active,
        "agent:worktrees sits before agent:recap:\n{updated}"
    );
    assert!(
        updated.contains(concat!(
            "- **Topic:** Earlier topic\n",
            "  - **Branch:** feat/earlier\n",
            "  - **Directory:** `/tmp/wt-a`\n",
            "- **Topic:** Ship the list\n",
            "  - **Branch:** feat/worktrees\n",
            "  - **Directory:** `/tmp/wt-b`\n",
        )),
        "{updated}"
    );
    let noted = updated.replace(
        "  - **Directory:** `/tmp/wt-a`\n",
        "  - **Directory:** `/tmp/wt-a`\n  - operator note\n",
    );
    assert!(
        reconcile(&noted, &records).unwrap().is_none(),
        "an operator child under a live directory is kept"
    );
    assert!(noted.contains("  - operator note\n"));
    let cleared = reconcile(&noted, &[]).unwrap().expect("stale rows clear");
    assert!(
        !cleared.contains("Earlier topic"),
        "a directory that is no longer live leaves the inventory:\n{cleared}"
    );
    assert!(
        !cleared.contains("operator note"),
        "a note leaves with its directory"
    );
    assert!(reconcile(&cleared, &[]).unwrap().is_none());
    assert!(
        reconcile(DOC, &[]).unwrap().is_none(),
        "no live worktree does not insert an empty section into an old document"
    );
}

#[test]
fn active_records_drop_removed_and_duplicate_directories() {
    let candidates = vec![
        WorktreeCandidate {
            topic: "first\nline two".to_string(),
            branch: "feat/one".to_string(),
            directory: "/wt/same".to_string(),
            generation: 1,
            active: true,
        },
        WorktreeCandidate {
            topic: "second".to_string(),
            branch: String::new(),
            directory: "/wt/same".to_string(),
            generation: 2,
            active: true,
        },
        WorktreeCandidate {
            topic: "removed".to_string(),
            branch: "feat/gone".to_string(),
            directory: "/wt/gone".to_string(),
            generation: 9,
            active: false,
        },
    ];
    assert_eq!(
        active_records(&candidates),
        vec![WorktreeRecord {
            topic: "second".to_string(),
            branch: "detached".to_string(),
            directory: "/wt/same".to_string(),
        }]
    );
}

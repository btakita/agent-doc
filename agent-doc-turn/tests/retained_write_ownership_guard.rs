//! `#percellconverge` architecture guard: one ownership predicate, three sites.
//!
//! Three crates tell an agent that a write is retained and that recovery is
//! forbidden. Their *wording* was already consolidated once — `crdt_relay_pending_refusal`'s
//! doc comment says it was made a single function "so all three call sites give
//! the same instruction rather than drifting into three dialects." That did not
//! work, because the thing that drifted was the **predicate**, not the prose:
//! each site went on asserting the deferral unconditionally, and 0.35.123 fixed
//! only one of them.
//!
//! The cost is not cosmetic. On 2026-08-03, on `tasks/agent-doc/agent-doc-bugs2.md`,
//! `write --commit` printed "this is a deferral, not a lost response — do NOT
//! re-send" while `session-check` printed "STRANDED, not deferred — waiting will
//! not commit them" about the same state. `session-check` was right all three
//! times, and each was recovered by hand with `agent-doc commit`. Whichever text
//! the agent reached **first** decided whether the work survived.
//!
//! So the rule is: a verdict's wording is authored exactly once, in
//! `agent_doc_turn::write_ownership`, where it is derived from the predicate. A
//! site that re-authors the phrase is a site that has re-acquired the ability to
//! disagree, which is the whole defect.
//!
//! This guard cannot check that a site passes *correct* facts — that is what the
//! per-site regressions are for. It checks the property a reviewer cannot hold in
//! their head: that no fourth dialect has appeared.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Phrases that state a verdict. Each belongs to exactly one branch of
/// `retained_write_remedy` and must not be authored anywhere else.
const VERDICT_PHRASES: [&str; 2] = ["deferral, not a lost response", "STRANDED, not deferred"];

/// The file allowed to author them.
const VERDICT_OWNER: &str = "agent-doc-turn/src/write_ownership.rs";

/// Every refusal site that retains a write and instructs the agent. Each must
/// reach the shared predicate rather than deciding for itself.
const REFUSAL_SITES: [(&str, &str); 3] = [
    (
        "agent-doc-git-io/src/live_buffer_guard.rs",
        "crdt_relay_pending_refusal",
    ),
    (
        "agent-doc-session-check-io/src/command.rs",
        "authority/disk divergence INTERRUPT",
    ),
    (
        "agent-doc-document-realtime-io/src/lib.rs",
        "await_editor_replica_no_disk_write",
    ),
];

/// Calls into the shared predicate. A site "reaches" it by calling one of these.
///
/// Deliberately the call forms with their `(`, and matched only on non-comment
/// lines. The first draft matched the bare module name anywhere in the file,
/// which a mutation probe walked straight through: deleting the call while
/// leaving a doc comment that *mentioned* `write_ownership` kept the guard
/// green. A guard satisfied by prose about the rule is not a guard.
const SHARED_PREDICATE_CALLS: [&str; 2] = ["retained_write_ownership(", "retained_write_remedy("];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives in the workspace root")
        .to_path_buf()
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // Build output, VCS, and non-Rust trees carry no workspace
                // source. `tests/` is skipped because a test may legitimately
                // assert the exact wording it expects to read.
                if matches!(
                    name.as_ref(),
                    "target" | ".git" | "editors" | "node_modules" | "tests" | "benches" | ".tsift"
                ) {
                    continue;
                }
                stack.push(path);
            } else if name.ends_with(".rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Line numbers (1-based) inside a `#[cfg(test)]` item, tracked by brace depth.
fn cfg_test_lines(source: &str) -> BTreeSet<usize> {
    let mut inside = BTreeSet::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0usize;
    while index < lines.len() {
        if !lines[index].trim_start().starts_with("#[cfg(test)]") {
            index += 1;
            continue;
        }
        let mut depth = 0i32;
        let mut opened = false;
        let mut cursor = index;
        while cursor < lines.len() {
            for ch in lines[cursor].chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            inside.insert(cursor + 1);
            if opened && depth <= 0 {
                break;
            }
            cursor += 1;
        }
        index = cursor + 1;
    }
    inside
}

/// A verdict's wording is authored once, where the predicate decides it.
#[test]
fn no_site_re_authors_a_retained_write_verdict() {
    let root = workspace_root();
    let mut findings = Vec::new();

    for path in rust_sources(&root) {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative == VERDICT_OWNER {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let test_lines = cfg_test_lines(&source);
        for (offset, line) in source.lines().enumerate() {
            let number = offset + 1;
            if test_lines.contains(&number) {
                continue;
            }
            for phrase in VERDICT_PHRASES {
                if line.contains(phrase) {
                    findings.push(format!("{relative}:{number}: re-authors `{phrase}`"));
                }
            }
        }
    }

    assert!(
        findings.is_empty(),
        "`#percellconverge`: a retained-write verdict must be authored only in `{VERDICT_OWNER}`, \
         where it is derived from `RetainedWriteOwnership::verdict`. Re-authoring the phrase is \
         how a site re-acquires the ability to contradict the others — on 2026-08-03 the write \
         path and `session-check` disagreed about the same state three times, and the agent \
         obeyed whichever it reached first.\n\nCall `agent_doc_turn::write_ownership::retained_write_remedy` \
         instead of writing the wording again.\n\n{}",
        findings.join("\n")
    );
}

/// Every refusal site reaches the shared predicate.
///
/// The complement of the rule above: consolidating the wording is worthless if a
/// site stops asking who owns the write.
#[test]
fn every_retained_write_refusal_site_reaches_the_shared_predicate() {
    let root = workspace_root();
    let mut missing = Vec::new();

    for (relative, site) in REFUSAL_SITES {
        let path = root.join(relative);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("refusal site {relative} must exist: {err}"));
        let reaches = source.lines().any(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                return false;
            }
            SHARED_PREDICATE_CALLS
                .iter()
                .any(|call| trimmed.contains(call))
        });
        if !reaches {
            missing.push(format!("{relative} ({site})"));
        }
    }

    assert!(
        missing.is_empty(),
        "`#percellconverge`: every site that retains a write and instructs the agent must ask \
         `agent_doc_turn::write_ownership` who owns it. A site that decides for itself will \
         eventually disagree with the others, and the agent obeys whichever it reaches first.\n\n\
         Sites not reaching the predicate:\n{}",
        missing.join("\n")
    );
}

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
const VERDICT_PHRASES: [&str; 4] = [
    "deferral, not a lost response",
    "STRANDED, not deferred",
    // `#ownershipverdictdiverges`: the third verdict. `write_applied` is neither
    // lost nor self-completing, and its phrase needs the same protection or a
    // site will eventually re-author it into a fourth dialect.
    "ALREADY LANDED",
    // `#strandedremedydeadlock`: the fourth. An unanswered document edit looks
    // exactly like a stranded write to an ownership check and takes the
    // opposite instruction, so its wording needs the same single owner.
    "UNANSWERED DOCUMENT EDIT",
];

/// The file allowed to author them.
const VERDICT_OWNER: &str = "agent-doc-turn/src/write_ownership.rs";

/// Every refusal site that retains a write and instructs the agent. Each must
/// reach the shared predicate rather than deciding for itself.
const REFUSAL_SITES: [(&str, &str); 5] = [
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
    // `#strandedremedydeadlock`: the fourth site is the one the other three
    // send the agent TO. `commit`'s already-current path held its own
    // predicate — any unproved typed-component drift was a terminal refusal —
    // so `session-check` could print "recover with `agent-doc commit <FILE>`"
    // and that command could answer "refusing to close as already committed"
    // about the same state. A remedy that names a command it cannot reach
    // agreement with is the same defect as a fourth wording dialect.
    (
        "agent-doc-commit-io/src/lib.rs",
        "already-current typed-component drift refusal",
    ),
    // `#commitwritecommitdeadlock`: the fifth. This one sits INSIDE `commit`
    // and used to send the agent to `write --commit`, which answers with the
    // `AwaitingTerminalCommit` remedy naming `commit` again. Adding a site to
    // this list is how a refusal stops being allowed to pick its remedy by
    // hand — the fourth site proved the pattern repeats, and it did.
    (
        "agent-doc-git-io/src/capture_materialization_guard.rs",
        "missing captured response refusal",
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

/// Constructors that BUILD a retained-write refusal the agent will read.
///
/// `#retainedwriteremedy`: the file-scoped check below is satisfied by a single
/// call anywhere in the file, so a NEW branch beside an existing one inherits a
/// pass it did not earn. That is exactly what happened —
/// `defer_visible_delivery_projection` emitted a retention message with no
/// remedy while its two sibling branches, in the same function, both appended
/// one. Its message ended "no secondary snapshot/commit or forced disk write was
/// attempted", which reads as *nothing happened*; the write had in fact applied
/// and needed only `agent-doc commit <FILE>`. Observed twice on 2026-08-09 on
/// `tasks/agent-doc/agent-doc-bugs2.md`, both recovered by that one command.
const RETENTION_ERROR_CONSTRUCTORS: [&str; 1] = ["await_editor_replica_no_disk_write("];

/// Call forms that reach the remedy, including a crate-local wrapper around it.
///
/// `retained_write_remedy_for(` does NOT contain `retained_write_remedy(` as a
/// substring, so the wrapper has to be named explicitly — matching only the bare
/// predicate would fail every site that routes through it.
const REMEDY_CALLS: [&str; 3] = [
    "retained_write_remedy(",
    "retained_write_remedy_for(",
    "retained_write_ownership(",
];

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

/// Every constructed retained-write refusal names its recovery.
///
/// The complement of the file-scoped check: reaching the predicate *somewhere*
/// in the file is not the property that matters to an agent reading one error
/// string. Each individual refusal it can receive must carry the remedy, or that
/// refusal reads as a lost response and invites the re-send `#percellconverge`
/// forbids.
#[test]
fn every_constructed_retained_write_refusal_names_its_remedy() {
    let root = workspace_root();
    let mut findings = Vec::new();

    for (relative, site) in REFUSAL_SITES {
        let path = root.join(relative);
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let test_lines = cfg_test_lines(&source);
        for constructor in RETENTION_ERROR_CONSTRUCTORS {
            let mut from = 0;
            while let Some(found) = source[from..].find(constructor) {
                let start = from + found;
                from = start + constructor.len();
                let line = source[..start].matches('\n').count() + 1;
                if test_lines.contains(&line) {
                    continue;
                }
                // Skip the definition itself and any comment mentioning it.
                let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
                let prefix = source[line_start..start].trim_start();
                if prefix.starts_with("//") || prefix.starts_with("fn ") || prefix.contains("fn ") {
                    continue;
                }
                let Some(argument) = balanced_argument(&source, from - 1) else {
                    continue;
                };
                let names_remedy = REMEDY_CALLS.iter().any(|call| argument.contains(call));
                if !names_remedy {
                    findings.push(format!(
                        "{relative}:{line}: `{constructor}` builds a refusal with no remedy ({site})"
                    ));
                }
            }
        }
    }

    assert!(
        findings.is_empty(),
        "`#retainedwriteremedy`: every constructed retained-write refusal must append the remedy \
         from `agent_doc_turn::write_ownership::retained_write_remedy`. A refusal that omits it \
         reads as \"nothing happened\" even when the write already applied and only the terminal \
         commit is outstanding — and an agent that believes the response was lost re-sends it, \
         which is the failure `#percellconverge` exists to prevent.\n\n{}",
        findings.join("\n")
    );
}

/// The text between the paren at `open` and its match, exclusive.
fn balanced_argument(source: &str, open: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    if bytes.get(open)? != &b'(' {
        return None;
    }
    let mut depth = 0i32;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return source.get(open + 1..open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

//! `#stategraphjoin` architecture guard.
//!
//! The rule this enforces is not "never call `Context::new()`" — a genuinely
//! standalone pure-transition helper legitimately does. It is that **every**
//! non-test construction of a reactive context must carry a written justification,
//! because the failure it prevents is invisible at runtime: a long-lived owner
//! holding a private context is an island, nothing outside can derive from its
//! cells, invalidation never crosses it, and a `Computed` built over it is Computed
//! in name only. That surfaces much later as a stale value, the most expensive
//! failure shape in this codebase.
//!
//! A reviewer cannot tell an island from a helper by looking at `Context::new()`.
//! Requiring the marker moves the question to write time, where the author still
//! knows the answer.
//!
//! To satisfy the guard, put the marker within [`MARKER_LOOKBACK_LINES`] lines above
//! the construction:
//!
//! ```text
//! // #stategraphjoin-allow: <why this is not an island>
//! let ctx = ThreadSafeContext::new();
//! ```
//!
//! If you cannot write a reason, that is the finding — join a scope
//! (`X::new_in(&DocumentScope, ..)`) instead.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// How far above a construction the guard looks for its justification marker.
const MARKER_LOOKBACK_LINES: usize = 6;

const ALLOW_MARKER: &str = "#stategraphjoin-allow:";

/// Bare reactive-context constructions.
const CONTEXT_CTORS: [&str; 4] = [
    "Context::new()",
    "ThreadSafeContext::new()",
    "AsyncContext::new()",
    "CycleContext::new()",
];

/// Pre-kernel reactive vocabulary (`#lzcellkernel`).
///
/// The current cell kernel is `Source` / `Computed` / `Effect`, read with `get`.
/// `signal` / `get_signal` / `SignalHandle` are the older two-node shape (a memoized
/// slot plus a puller effect). They still compile, which is exactly why a guard is
/// needed: nothing else stops a new caller from reaching for them.
///
/// `ctx.signal(` rather than `.signal(` on purpose — `stop.signal()` is an unrelated
/// stop-flag API and must not trip this.
const DEPRECATED_REACTIVE_API: [&str; 3] = ["ctx.signal(", "get_signal(", "SignalHandle"];

const DEPRECATED_ALLOW_MARKER: &str = "#lzcellkernel-allow:";

struct Finding {
    file: PathBuf,
    line: usize,
    text: String,
    pattern: &'static str,
}

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
                // Build output, VCS, and non-Rust editor trees carry no workspace
                // source. `tests/` is skipped for the same reason `#[cfg(test)]` is:
                // a test fixture graph is standalone by construction.
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

/// Line numbers (1-based) that fall inside a `#[cfg(test)]` item.
///
/// Tracked by brace depth from the attribute's item, so nested modules and
/// `#[cfg(test)]` functions are both covered, and code after the block is not.
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

fn scan(patterns: &[&'static str], marker: &str, skip: &dyn Fn(&Path) -> bool) -> Vec<Finding> {
    let root = workspace_root();
    let mut findings = Vec::new();
    for file in rust_sources(&root) {
        if skip(&file) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        let test_lines = cfg_test_lines(&source);
        let lines: Vec<&str> = source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            let number = index + 1;
            if test_lines.contains(&number) {
                continue;
            }
            let Some(pattern) = patterns.iter().find(|pattern| line.contains(**pattern)) else {
                continue;
            };
            // The marker itself, and doc/comment prose naming the pattern, are not
            // constructions. Only real code is.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            let start = number.saturating_sub(MARKER_LOOKBACK_LINES).max(1);
            let justified = lines[start - 1..number]
                .iter()
                .any(|candidate| candidate.contains(marker));
            if justified {
                continue;
            }
            findings.push(Finding {
                file: file
                    .strip_prefix(&root)
                    .unwrap_or(&file)
                    .to_path_buf(),
                line: number,
                text: line.trim().to_string(),
                pattern,
            });
        }
    }
    findings
}

fn report(findings: &[Finding], rule: &str, how_to_fix: &str) -> String {
    let mut out = format!(
        "{} — {} unjustified site(s).\n\n{}\n\n",
        rule,
        findings.len(),
        how_to_fix
    );
    for finding in findings {
        out.push_str(&format!(
            "  {}:{}  [{}]\n      {}\n",
            finding.file.display(),
            finding.line,
            finding.pattern,
            finding.text
        ));
    }
    out
}

/// A new bare reactive context outside a test must carry a written justification.
///
/// This is the guard the `#stategraphjoin` audit asked for. It cannot decide whether
/// a given context is an island — that needs the author's intent — so it demands the
/// intent be written down, which is the part that was missing when nine state
/// machines each minted their own context and nobody noticed for a year.
#[test]
fn no_unjustified_bare_reactive_context() {
    let findings = scan(&CONTEXT_CTORS, ALLOW_MARKER, &|file| {
        // This crate DEFINES the scopes; its own contexts are the ones every other
        // crate joins.
        file.components()
            .any(|part| part.as_os_str() == "agent-doc-state-scope")
    });

    assert!(
        findings.is_empty(),
        "{}",
        report(
            &findings,
            "#stategraphjoin",
            "Join a scope instead: `X::new_in(&DocumentScope, ..)` / `&TurnScope` / `&ProcessScope`.\n\
             If the context genuinely belongs to a standalone pure helper or a bounded\n\
             per-call transform, say so on the line above:\n\
             \n    // #stategraphjoin-allow: <why this is not an island>\n\
             \n\"It is only a small context\" is not a reason — an island is an island at any size.",
        )
    );
}

/// The reactive vocabulary is `Source` / `Computed` / `Effect`, read with `get`.
///
/// `signal` / `get_signal` / `SignalHandle` still compile, so without this nothing
/// stops a new caller from reaching for the pre-kernel shape — which is exactly how
/// it got reached for.
#[test]
fn no_deprecated_reactive_vocabulary() {
    let findings = scan(&DEPRECATED_REACTIVE_API, DEPRECATED_ALLOW_MARKER, &|_| false);

    assert!(
        findings.is_empty(),
        "{}",
        report(
            &findings,
            "#lzcellkernel",
            "Derive with `ctx.computed(..)` and read it with `ctx.get(&cell)`.\n\
             `signal`/`get_signal` are the pre-kernel two-node shape (memo slot plus a\n\
             puller effect) and are not the vocabulary this codebase derives in.\n\
             If a call genuinely needs eager pull semantics the thread-safe `Computed`\n\
             cannot express yet, say so on the line above:\n\
             \n    // #lzcellkernel-allow: <why the eager puller is required here>",
        )
    );
}

/// The guard must actually detect the shapes it claims to — a scanner that silently
/// matches nothing reports green forever.
///
/// Mutation-checks both directions: an unjustified construction is found, and the
/// marker (and `#[cfg(test)]`) genuinely suppress it.
#[test]
fn the_guard_detects_what_it_claims_to() {
    assert!(
        cfg_test_lines("#[cfg(test)]\nmod tests {\n    let ctx = Context::new();\n}\nlet after = 1;\n")
            .contains(&3),
        "a construction inside #[cfg(test)] must be recognized as test code"
    );
    assert!(
        !cfg_test_lines("#[cfg(test)]\nmod tests {\n    let a = 1;\n}\nlet ctx = Context::new();\n")
            .contains(&5),
        "code after the test block must NOT be treated as test code"
    );
    assert!(
        CONTEXT_CTORS
            .iter()
            .any(|pattern| "        let ctx = ThreadSafeContext::new();".contains(pattern)),
        "the real construction shape must match a pattern"
    );
    assert!(
        !DEPRECATED_REACTIVE_API
            .iter()
            .any(|pattern| "        stop.signal();".contains(pattern)),
        "the unrelated stop-flag API must not trip the deprecated-vocabulary guard"
    );
    assert!(
        DEPRECATED_REACTIVE_API
            .iter()
            .any(|pattern| "        let x = ctx.signal(|ctx| 1);".contains(pattern)),
        "the pre-kernel derivation shape must match a pattern"
    );
}

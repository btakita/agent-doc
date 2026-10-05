//! Replay the committed fuzz corpus and every promoted crasher through the
//! harness functions on the stable toolchain (`#netadv7`).
//!
//! `fuzz/corpus/<target>/` holds the redacted seed corpus (real traffic shapes)
//! and `fuzz/regressions/<target>/` holds minimized inputs that once crashed a
//! decoder. Both run under `make check`, so a regression of any fixed crasher
//! fails the ordinary test suite without nightly or cargo-fuzz.

use std::path::{Path, PathBuf};

fn fuzz_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz")
}

fn inputs(kind: &str, target: &str) -> Vec<PathBuf> {
    let dir = fuzz_dir().join(kind).join(target);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    files
}

fn replay(kind: &str, target: &str) -> usize {
    let run = agent_doc_fuzz_harness::target(target).expect("known fuzz target");
    let files = inputs(kind, target);
    for path in &files {
        let data = std::fs::read(path).expect("read corpus input");
        let outcome = std::panic::catch_unwind(|| run(&data));
        assert!(
            outcome.is_ok(),
            "{target}: input {} violated a harness oracle",
            path.display()
        );
    }
    files.len()
}

#[test]
fn every_target_has_a_seed_corpus() {
    for (target, _) in agent_doc_fuzz_harness::TARGETS {
        assert!(
            !inputs("corpus", target).is_empty(),
            "fuzz/corpus/{target}/ must carry at least one seed"
        );
    }
}

#[test]
fn replay_ipc_wire() {
    assert!(replay("corpus", "ipc_wire") > 0);
    replay("regressions", "ipc_wire");
}

#[test]
fn replay_markdown_patch() {
    assert!(replay("corpus", "markdown_patch") > 0);
    replay("regressions", "markdown_patch");
}

#[test]
fn replay_frontmatter() {
    assert!(replay("corpus", "frontmatter") > 0);
    replay("regressions", "frontmatter");
}

#[test]
fn replay_crdt_update() {
    assert!(replay("corpus", "crdt_update") > 0);
    replay("regressions", "crdt_update");
}

#[test]
fn replay_crdt_edits() {
    assert!(replay("corpus", "crdt_edits") > 0);
    replay("regressions", "crdt_edits");
}

#[test]
fn every_fuzz_target_file_is_registered_in_the_harness() {
    let dir = fuzz_dir().join("fuzz_targets");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("fuzz/fuzz_targets exists")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .path()
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
        .collect();
    names.sort();
    let mut registered: Vec<String> = agent_doc_fuzz_harness::TARGETS
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    registered.sort();
    assert_eq!(names, registered, "fuzz/fuzz_targets and TARGETS drifted");
}

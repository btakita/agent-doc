//! `#ipcverhandshake` — the IPC build identity must be derived from the code,
//! never from a clock.
//!
//! The handshake rejects a peer whose `build_id` differs and every caller reads
//! that rejection as proof of differing code. While the identity was
//! `SystemTime::now()` at build-script time the proof did not exist in either
//! direction: identical sources built twice were rejected (which strands an
//! attached document's replica with `operator_action=none`), and an edit to any
//! member package was admitted (a build script is never re-run for a change in
//! a different package). These guards pin the producer so the clock cannot come
//! back quietly. The model of both directions is
//! `formal/tla/IpcBuildIdentity.tla`.

use std::path::{Path, PathBuf};

/// Exactly what `src/main.rs` and `src/ffi.rs` hand to
/// `agent_doc_ipc_io::set_local_build_id`.
const WIRE_BUILD_ID: &str = concat!(env!("CARGO_PKG_VERSION"), "+", env!("AGENT_DOC_BUILD_ID"));

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn build_suffix() -> &'static str {
    WIRE_BUILD_ID
        .split_once('+')
        .expect("the wire build id is <version>+<build id>")
        .1
}

/// The regression pin. A unix-seconds stamp is all digits; a content digest is
/// hex of a fixed width. Reintroducing the clock reddens here rather than in
/// production three installs later.
#[test]
fn the_wire_build_identity_is_a_content_digest_not_a_timestamp() {
    let suffix = build_suffix();
    assert_eq!(
        suffix.len(),
        agent_doc_hash::SOURCE_DIGEST_HEX_LEN,
        "build identity {suffix} is not a {}-character digest",
        agent_doc_hash::SOURCE_DIGEST_HEX_LEN
    );
    assert!(
        suffix.chars().all(|c| c.is_ascii_hexdigit()),
        "build identity {suffix} is not hex"
    );
    assert!(
        suffix.parse::<u64>().is_err(),
        "build identity {suffix} still reads as a bare numeric timestamp"
    );
}

/// The identity must be a function of the tree alone. Two digests taken of the
/// same tree at different moments are what the old stamp got wrong, so the
/// property is asserted directly rather than inferred from the shape.
#[test]
fn the_workspace_digest_does_not_depend_on_when_it_is_taken() {
    let root = workspace_root();
    let first = agent_doc_hash::workspace_source_digest(&root).expect("digest the workspace");
    let second = agent_doc_hash::workspace_source_digest(&root).expect("digest the workspace");
    assert_eq!(first, second);
}

/// The build script is the producer; this pins it to the same helper the guards
/// above reason about, and forbids the spellings that made it a clock.
///
/// Scanned with comments stripped. The prose in `build.rs` names the old
/// timestamp on purpose — a fix whose reason is deleted is a fix waiting to be
/// reverted — and a guard that cannot tell an explanation from an instruction
/// would force that deletion. (It reddened on exactly that, which is the guard
/// working.)
#[test]
fn the_build_script_derives_the_identity_from_the_sources() {
    let build_rs = std::fs::read_to_string(workspace_root().join("build.rs"))
        .expect("the root package has a build script");
    let code = strip_comments(&build_rs);
    assert!(
        code.contains("workspace_source_digest"),
        "build.rs no longer derives the identity from the workspace sources"
    );
    assert!(
        code.contains("AGENT_DOC_BUILD_ID"),
        "build.rs no longer emits AGENT_DOC_BUILD_ID"
    );
    for banned in ["SystemTime", "UNIX_EPOCH", "AGENT_DOC_BUILD_TIMESTAMP"] {
        assert!(
            !code.contains(banned),
            "build.rs executes {banned}: the build identity is drifting back to a clock"
        );
    }
}

fn strip_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(start) => &line[..start],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `rerun-if-changed=src/` covered one directory of a workspace whose behaviour
/// lives in ~140 member packages, so the stamp survived the edits it was
/// supposed to notice. The declaration must come from the digest's own input
/// enumeration, and that enumeration has to reach the IPC crates.
#[test]
fn the_declared_inputs_reach_the_member_packages() {
    let root = workspace_root();
    let inputs = agent_doc_hash::source_digest_inputs(&root).expect("enumerate digest inputs");
    let declared: Vec<String> = inputs
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();

    for required in [
        "Cargo.lock",
        "Cargo.toml",
        "src/main.rs",
        "src/ffi.rs",
        "agent-doc-ipc-io/src/lib.rs",
        "agent-doc-ipc-protocol/src/lib.rs",
        "agent-doc-crdt-relay-io/src/lib.rs",
    ] {
        assert!(
            declared.iter().any(|input| input == required),
            "{required} is outside the build identity, so a change to it would handshake as identical"
        );
    }

    // The escape hatch stays narrow: only trees that cannot be linked into the
    // binary are allowed to be invisible.
    assert!(
        !declared
            .iter()
            .any(|input| input.starts_with("target/") || input.contains("/target/")),
        "build artifacts must not reach the build identity"
    );
}

/// A changed member package must move the identity. Exercised against a
/// throwaway copy of a real member crate's path shape rather than the live tree,
/// so the guard cannot be satisfied by an unrelated concurrent edit.
#[test]
fn a_member_package_edit_moves_the_identity() {
    let scratch = tempfile::TempDir::new().expect("scratch workspace");
    let root = scratch.path();
    write(root, "Cargo.toml", "[workspace]\nmembers = [\"agent-doc-ipc-io\"]\n");
    write(root, "src/main.rs", "fn main() {}\n");
    write(
        root,
        "agent-doc-ipc-io/Cargo.toml",
        "[package]\nname = \"agent-doc-ipc-io\"\n",
    );
    write(
        root,
        "agent-doc-ipc-io/src/lib.rs",
        "pub const IPC_PROTOCOL_VERSION: u32 = 1;\n",
    );

    let before = agent_doc_hash::workspace_source_digest(root).expect("digest");
    write(
        root,
        "agent-doc-ipc-io/src/lib.rs",
        "pub const IPC_PROTOCOL_VERSION: u32 = 2;\n",
    );
    let after = agent_doc_hash::workspace_source_digest(root).expect("digest");

    assert_ne!(
        before, after,
        "a protocol change in a member package handshakes as the same build"
    );
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("relative path has a parent"))
        .expect("create scratch directories");
    std::fs::write(path, content).expect("write scratch source");
}

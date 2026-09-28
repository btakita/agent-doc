//! Emits the IPC build identity.
//!
//! This used to emit `AGENT_DOC_BUILD_TIMESTAMP` — `SystemTime::now()` at build
//! script run time — and declare `rerun-if-changed=src/`. The handshake treats a
//! differing identity as proof that two processes run different code, and a
//! clock cannot carry that proof: identical sources built twice got different
//! stamps (a spurious rejection, which wedges an attached document with no
//! operator remedy), while an edit to any of the ~140 member packages got the
//! same stamp carried over, because a cargo build script is never re-run for a
//! change in a different package. See
//! `agent-doc-hash/src/source_digest.rs` for the measured evidence and
//! `formal/tla/IpcBuildIdentity.tla` for both failure directions as a model.
//!
//! The identity is now the digest of the build-relevant sources themselves, and
//! the `rerun-if-changed` set is the same enumeration the digest is taken over
//! rather than a hand-written guess at it.

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );

    let inputs = agent_doc_hash::source_digest_inputs(&root)
        .unwrap_or_else(|error| panic!("failed to enumerate build identity inputs: {error}"));
    let build_id = agent_doc_hash::workspace_source_digest(&root)
        .unwrap_or_else(|error| panic!("failed to compute the build identity: {error}"));

    // One declaration per hashed file, plus the directory holding it so that
    // adding or removing a source also re-runs this script. A file-only
    // declaration cannot observe creation.
    let mut directories = std::collections::BTreeSet::from([".".to_string()]);
    for relative in &inputs {
        println!("cargo:rerun-if-changed={}", relative.display());
        let parent = relative.parent().map(|parent| parent.display().to_string());
        if let Some(parent) = parent.filter(|parent| !parent.is_empty()) {
            directories.insert(parent);
        }
    }
    for directory in directories {
        println!("cargo:rerun-if-changed={directory}");
    }

    println!("cargo:rustc-env=AGENT_DOC_BUILD_ID={build_id}");
}

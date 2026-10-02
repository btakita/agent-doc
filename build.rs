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

    write_source_anchors(&root, &inputs);
}

/// GH 92: the `#anchor` tokens agent-doc's own Rust comments define, written to
/// `$OUT_DIR/source_anchors.txt` for `agent_doc_fs::register_source_anchors`.
///
/// Taken over the same enumeration as the build identity, so it is re-derived
/// exactly when the sources are. Deliberately loose — only `//` comment text,
/// only `#` not glued to a preceding word — because the runtime re-parses the
/// tokens through `extract_tags`, the single tag grammar; this only has to avoid
/// shipping string-literal and code noise.
fn write_source_anchors(root: &std::path::Path, inputs: &[PathBuf]) {
    let mut tokens = std::collections::BTreeSet::new();
    for relative in inputs {
        if relative.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(root.join(relative)) else {
            continue;
        };
        for line in content.lines() {
            let Some(comment) = comment_text(line) else {
                continue;
            };
            let chars: Vec<char> = comment.chars().collect();
            for (index, ch) in chars.iter().enumerate() {
                if *ch != '#' {
                    continue;
                }
                if index > 0
                    && (chars[index - 1].is_alphanumeric()
                        || chars[index - 1] == '_'
                        || chars[index - 1] == '-')
                {
                    continue;
                }
                let token: String = chars[index + 1..]
                    .iter()
                    .take_while(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || **c == '-' || **c == '_'
                    })
                    .collect();
                if token.len() >= 2 {
                    tokens.insert(token);
                }
            }
        }
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let body = tokens
        .iter()
        .map(|token| format!("#{token}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(out.join("source_anchors.txt"), body)
        .unwrap_or_else(|error| panic!("failed to write source anchors: {error}"));
}

/// The comment portion of a source line: a whole-line `//` comment, or a
/// trailing ` // ` comment outside any string literal (even quote count).
fn comment_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("//") {
        return Some(rest);
    }
    let index = line.find(" // ")?;
    line[..index]
        .matches('"')
        .count()
        .is_multiple_of(2)
        .then(|| &line[index + 4..])
}

//! A content-derived build identity for the workspace.
//!
//! # Why this exists
//!
//! The IPC handshake (`#ipcverhandshake`) admits a peer only when both sides
//! advertise the same `build_id`, and every caller reads a rejection as proof
//! that the two processes run different code. That reading was never earned:
//! the identity was `CARGO_PKG_VERSION + "+" + <wall-clock seconds at build
//! script run>`, which is a statement about *when* a binary was built, not
//! about *what* is in it. A clock cannot decide code equality, and it failed in
//! both directions at once.
//!
//! **False mismatch.** Two builds of byte-identical source made at different
//! moments get different stamps. That is not a corner case — it is the normal
//! workflow. `cargo install --path .` and `cargo build --release` use different
//! target directories, so each runs its own build script and stamps its own
//! clock; so does a `cargo clean`, a second checkout, or a CI build of the same
//! commit. The handshake then rejects two processes that behave identically,
//! and the rejection is terminal: the editor replica cannot be re-registered,
//! `editor_attached_model_missing` survives its bounded observation, and
//! `read_command_document` ends at
//! `realtime_doc_resolve_missing_replica_terminal_rebuild_failed` +
//! `realtime_doc_resolve_disk_read_refused` with `operator_action=none`. The
//! document is unreachable, for a difference that does not exist.
//! `formal/tla/EditorReplicaStrand.tla` proves that resting state is a genuine
//! state-graph deadlock; `formal/tla/IpcBuildIdentity.tla` proves a clock-derived
//! identity is what reaches it.
//!
//! **False match.** The build script declared `rerun-if-changed=src/`, naming
//! only the root package's own sources. A cargo build script never re-runs for
//! changes inside a *different* package, and every workspace member is a
//! different package — which is where essentially all agent-doc behaviour
//! lives, IPC included. So editing `agent-doc-ipc-io` or
//! `agent-doc-crdt-relay-io` rebuilt the binary while the stamp was carried
//! over verbatim, and two processes with genuinely different wire behaviour
//! handshook as identical. Measured 2026-09-28: `~/.cargo/bin/agent-doc` was
//! written at 23:51:45 and advertised `+1790563363` (22:42:43), having been
//! built from a tree that changed `agent-doc-crdt-relay-io/src/lib.rs` at
//! 23:25. That is the exact property the handshake exists to enforce, silently
//! unenforced for the majority of the codebase.
//!
//! # What replaces it
//!
//! [`workspace_source_digest`] hashes the build-relevant sources themselves, so
//! the identity is a function of the code and of nothing else:
//!
//! * same sources ⇒ same id, whatever the clock, the target directory or the
//!   checkout path said (no false mismatch);
//! * any change to any compiled source in any member crate ⇒ different id (no
//!   false match).
//!
//! [`source_digest_inputs`] returns the same file set the digest was taken
//! over, so the build script can emit one `rerun-if-changed` per input from the
//! *same* enumeration. Deriving the two separately is how the old declaration
//! drifted from what it was supposed to cover.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// Length of the hex build identity. 64 bits of SHA-256 — short enough to read
/// in a log line, wide enough that an accidental collision between two source
/// trees is not a thing that happens.
pub const SOURCE_DIGEST_HEX_LEN: usize = 16;

/// Directories that never contribute to the compiled binary, skipped wherever
/// they appear. `target` and `.git` are build/VCS state, `editors` is the JVM
/// and TypeScript plugin sources, `docs` is prose, and `.worktrees` is other
/// checkouts of this same repository.
const EXCLUDED_DIRECTORIES: &[&str] = &[
    "target",
    ".git",
    ".agent-doc",
    ".worktrees",
    "node_modules",
    "editors",
    "docs",
    "formal",
];

/// Directories excluded only directly beneath a package root (a directory
/// holding a `Cargo.toml`). These are cargo's conventional locations for
/// targets that are never linked into the shipped binary or cdylib, so a change
/// there cannot alter what a peer does on the wire.
///
/// Deliberately scoped to a package root rather than matched by name anywhere:
/// a `tests` module *inside* `src/` is compiled code, and skipping it would
/// reopen the false-match hole this module exists to close.
const PACKAGE_LOCAL_EXCLUDED_DIRECTORIES: &[&str] = &["tests", "benches", "examples"];

/// Non-`.rs` files whose content changes what the binary does.
const INCLUDED_FILE_NAMES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain",
    "rust-toolchain.toml",
];

/// The build-relevant source files under `root`, as workspace-relative paths in
/// a deterministic order.
///
/// Relative and `/`-separated, so the same tree digests identically no matter
/// where it is checked out — which is the whole point, since two target
/// directories for one checkout were enough to manufacture a mismatch.
pub fn source_digest_inputs(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut inputs = BTreeMap::new();
    collect_inputs(root, root, &mut inputs)?;
    Ok(inputs.into_values().collect())
}

/// The content-derived build identity for the workspace rooted at `root`.
///
/// The preimage names each file as well as hashing it, so moving code between
/// files changes the identity even when the bytes are conserved.
pub fn workspace_source_digest(root: &Path) -> io::Result<String> {
    let mut preimage = String::new();
    for relative in source_digest_inputs(root)? {
        let key = normalized_relative_path(&relative);
        let bytes = std::fs::read(root.join(&relative))?;
        preimage.push_str(&key);
        preimage.push('\0');
        preimage.push_str(&bytes.len().to_string());
        preimage.push('\0');
        preimage.push_str(&crate::bytes_hash(&bytes));
        preimage.push('\n');
    }
    let mut digest = crate::content_hash(&preimage);
    digest.truncate(SOURCE_DIGEST_HEX_LEN);
    Ok(digest)
}

fn collect_inputs(
    root: &Path,
    directory: &Path,
    inputs: &mut BTreeMap<String, PathBuf>,
) -> io::Result<()> {
    let is_package_root = directory.join("Cargo.toml").is_file();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // `file_type` reports the link itself; resolve it so a symlinked source
        // file is still hashed while a symlinked directory is not descended
        // into (that is how a walk finds its own cycle).
        let file_type = entry.file_type()?;
        let is_directory = if file_type.is_symlink() {
            match std::fs::metadata(&path) {
                Ok(metadata) => metadata.is_dir(),
                // A dangling symlink contributes nothing to a build.
                Err(_) => continue,
            }
        } else {
            file_type.is_dir()
        };
        if is_directory {
            if file_type.is_symlink() || excluded_directory(&name, is_package_root) {
                continue;
            }
            collect_inputs(root, &path, inputs)?;
        } else if included_file(&name) {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            inputs.insert(normalized_relative_path(&relative), relative);
        }
    }
    Ok(())
}

fn excluded_directory(name: &str, parent_is_package_root: bool) -> bool {
    EXCLUDED_DIRECTORIES.contains(&name)
        || (parent_is_package_root && PACKAGE_LOCAL_EXCLUDED_DIRECTORIES.contains(&name))
}

fn included_file(name: &str) -> bool {
    name.ends_with(".rs") || INCLUDED_FILE_NAMES.contains(&name)
}

fn normalized_relative_path(relative: &Path) -> String {
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::{SOURCE_DIGEST_HEX_LEN, source_digest_inputs, workspace_source_digest};
    use std::path::Path;
    use tempfile::TempDir;

    /// A minimal two-package workspace: a root package and one member, which is
    /// the shape the old `rerun-if-changed=src/` declaration could not see past.
    fn workspace(root: &Path) {
        write(root, "Cargo.toml", "[workspace]\nmembers = [\"member\"]\n");
        write(root, "Cargo.lock", "# lock\n");
        write(root, "src/main.rs", "fn main() {}\n");
        write(root, "tests/integration.rs", "#[test] fn t() {}\n");
        write(root, "member/Cargo.toml", "[package]\nname = \"member\"\n");
        write(root, "member/src/lib.rs", "pub fn ipc() -> u8 { 1 }\n");
        write(root, "member/tests/wire.rs", "#[test] fn t() {}\n");
        write(root, "target/debug/build/generated.rs", "pub fn stale() {}\n");
    }

    fn write(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn digest(root: &Path) -> String {
        workspace_source_digest(root).unwrap()
    }

    /// The false-mismatch half. Nothing about *when* or *where* a tree was built
    /// may reach the identity: rewriting every byte identically at a later time,
    /// in a different directory, must digest the same. A clock-derived stamp
    /// fails this, and each failure was an unrecoverable document wedge.
    #[test]
    fn identical_sources_digest_identically_regardless_of_time_or_location() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        workspace(first.path());
        workspace(second.path());
        // Rewrite in place so mtimes advance without content changing.
        write(first.path(), "member/src/lib.rs", "pub fn ipc() -> u8 { 1 }\n");
        assert_eq!(digest(first.path()), digest(second.path()));
    }

    /// The false-match half, and the one that would have caught the shipped
    /// bug: the edit is in a *member* package, which no root build script is
    /// ever re-run for.
    #[test]
    fn a_member_crate_edit_changes_the_digest() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        let before = digest(root.path());
        write(root.path(), "member/src/lib.rs", "pub fn ipc() -> u8 { 2 }\n");
        assert_ne!(before, digest(root.path()));
    }

    /// A dependency version change alters behaviour without touching a `.rs`
    /// file, so the manifests are part of the identity too.
    #[test]
    fn a_manifest_or_lockfile_edit_changes_the_digest() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        let before = digest(root.path());
        write(root.path(), "Cargo.lock", "# lock\n# bumped\n");
        assert_ne!(before, digest(root.path()));
        let bumped = digest(root.path());
        write(root.path(), "member/Cargo.toml", "[package]\nname = \"member\"\nversion = \"2\"\n");
        assert_ne!(bumped, digest(root.path()));
    }

    /// Moving code between files changes what each file is, so the preimage
    /// names every path and not only its bytes.
    #[test]
    fn relocating_identical_bytes_changes_the_digest() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        let before = digest(root.path());
        std::fs::rename(
            root.path().join("member/src/lib.rs"),
            root.path().join("member/src/wire.rs"),
        )
        .unwrap();
        assert_ne!(before, digest(root.path()));
    }

    /// Adding a compiled source must move the identity even though no existing
    /// file changed.
    #[test]
    fn adding_a_source_file_changes_the_digest() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        let before = digest(root.path());
        write(root.path(), "member/src/extra.rs", "pub fn extra() {}\n");
        assert_ne!(before, digest(root.path()));
    }

    /// Integration tests and build artifacts are not linked into the binary, so
    /// churning them must not force the whole fleet to re-handshake. This is the
    /// only exclusion that is allowed to hide a change, and it is scoped to a
    /// package root — a `tests` module inside `src/` stays in the digest.
    #[test]
    fn excluded_trees_do_not_reach_the_digest() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        let before = digest(root.path());
        write(root.path(), "tests/integration.rs", "#[test] fn changed() {}\n");
        write(root.path(), "member/tests/wire.rs", "#[test] fn changed() {}\n");
        write(root.path(), "target/debug/build/generated.rs", "pub fn changed() {}\n");
        assert_eq!(before, digest(root.path()));
    }

    #[test]
    fn a_tests_module_inside_src_stays_in_the_digest() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        write(root.path(), "member/src/tests/mod.rs", "pub fn helper() -> u8 { 1 }\n");
        let before = digest(root.path());
        write(root.path(), "member/src/tests/mod.rs", "pub fn helper() -> u8 { 2 }\n");
        assert_ne!(before, digest(root.path()));
    }

    /// The build script declares one `rerun-if-changed` per input, so the file
    /// set it declares and the file set it hashes have to be the same
    /// enumeration. They were separate before, and that is how the declaration
    /// came to cover a single directory of a forty-package workspace.
    #[test]
    fn declared_inputs_are_exactly_the_hashed_inputs() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        let inputs: Vec<String> = source_digest_inputs(root.path())
            .unwrap()
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            inputs,
            vec![
                "Cargo.lock".to_string(),
                "Cargo.toml".to_string(),
                "member/Cargo.toml".to_string(),
                "member/src/lib.rs".to_string(),
                "src/main.rs".to_string(),
            ]
        );
    }

    #[test]
    fn the_digest_is_hex_of_the_declared_width() {
        let root = TempDir::new().unwrap();
        workspace(root.path());
        let digest = digest(root.path());
        assert_eq!(digest.len(), SOURCE_DIGEST_HEX_LEN);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

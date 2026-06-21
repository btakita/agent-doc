//! # Module: op_capture (`#qnodemerge4wire`)
//!
//! Supply side of the op-capture / evented-reflection merge (`#qnodemerge4`).
//! The *consumer* (`agent_doc_core::crdt::merge_with_editor_ops` + `EditorOp` /
//! `replay_editor_ops`) already trusts the editor's *real* operations over a
//! Myers diff-guess when those ops replay onto the merge base exactly. This
//! module is the durable plumbing that gets those ops from the editor to the
//! merge:
//!
//! - **Persistence** — a per-document sidecar at
//!   `.agent-doc/op-capture/<doc-hash>.json` holding the ordered ops the editor
//!   reported, plus the `base_hash` of the buffer text they were captured
//!   *against*. Keying by base hash is the safety anchor: ops are only ever
//!   trusted by a merge whose resolved base hashes to the same value, so a
//!   stale/advanced base silently disqualifies them (the merge falls back to
//!   the diff-guess — never worse than today).
//! - **Append discipline** — recording an op against a *different* base than
//!   the one currently stored starts a fresh epoch (the prior ops were captured
//!   against a base that no longer applies).
//! - **Consume + clear** — once a merge has loaded the ops for its base they are
//!   cleared, so the next epoch starts clean and stale ops can never leak into a
//!   later, unrelated merge.
//!
//! ## Agentic Contracts
//! - `record_editor_op` is append-only within an epoch and resets the epoch when
//!   `base_hash` changes; it never silently drops an op.
//! - `editor_ops_for_base` returns `Some(ops)` **only** when the stored
//!   `base_hash` matches the hash of the supplied base text; otherwise `None`.
//! - `clear_op_capture` is idempotent — clearing an absent sidecar is `Ok(())`.
//! - `content_hash` is the single source of the base-hash function shared by the
//!   producer (editor, via FFI) and the consumer (merge).
//!
//! Wiring point (`#qnodemerge4wire` part 3): `merge::merge_contents_crdt_with_ops`
//! loads ops via `editor_ops_for_base`, passes them to `merge_with_editor_ops`,
//! and clears the sidecar after the merge.

use crate::fs_util::read_optional_text;
use agent_doc_core::crdt::EditorOp;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const OP_CAPTURE_SUBDIR: &str = ".agent-doc/op-capture";

/// The per-document op-capture sidecar.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpCaptureSidecar {
    /// SHA256 hex of the buffer text the ops were captured against — the merge
    /// base they replay onto. The merge trusts the ops only when its resolved
    /// base hashes to this value.
    pub base_hash: String,
    /// Ordered editor operations, in the sequence the editor performed them.
    pub ops: Vec<EditorOp>,
    /// Last-update wall-clock (ms since epoch), best-effort, for GC.
    #[serde(default)]
    pub updated_ms: u128,
}

/// Compute the SHA256 hex hash of arbitrary text — the base-hash function shared
/// by the producer (editor reporters, via FFI) and the consumer (merge).
pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

/// Path to the op-capture sidecar for `doc`:
/// `<project_root>/.agent-doc/op-capture/<doc-hash>.json`.
fn op_capture_path_for(doc: &Path) -> Result<PathBuf> {
    let canonical = doc.canonicalize()?;
    let hash = crate::snapshot::doc_hash(&canonical)?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok(project_root
        .join(OP_CAPTURE_SUBDIR)
        .join(format!("{hash}.json")))
}

/// Load the op-capture sidecar for `doc`, if present.
///
/// A malformed sidecar is treated as absent (logged) rather than failing the
/// merge — a corrupt capture must never block a write.
pub fn load_op_capture(doc: &Path) -> Result<Option<OpCaptureSidecar>> {
    let path = op_capture_path_for(doc)?;
    let Some(json) = read_optional_text(&path)? else {
        return Ok(None);
    };
    match serde_json::from_str::<OpCaptureSidecar>(&json) {
        Ok(sidecar) => Ok(Some(sidecar)),
        Err(e) => {
            eprintln!(
                "[op-capture] ignoring malformed sidecar {} ({e}); falling back to diff-guess",
                path.display()
            );
            Ok(None)
        }
    }
}

/// Record one editor op for `doc`, captured against the buffer whose text hashes
/// to `base_hash`.
///
/// Appends within the current epoch when `base_hash` matches the stored one;
/// otherwise starts a fresh epoch (the prior ops were captured against a base
/// that no longer applies, so they are discarded).
pub fn record_editor_op(doc: &Path, base_hash: &str, op: EditorOp) -> Result<()> {
    let mut sidecar = match load_op_capture(doc)? {
        Some(existing) if existing.base_hash == base_hash => existing,
        _ => OpCaptureSidecar {
            base_hash: base_hash.to_string(),
            ops: Vec::new(),
            updated_ms: 0,
        },
    };
    sidecar.ops.push(op);
    sidecar.updated_ms = now_millis();
    write_sidecar(doc, &sidecar)
}

/// Return the captured ops for `doc` **only** when they were captured against a
/// base whose text matches `base_text` (by hash). A mismatch (stale/advanced
/// base, missed event) returns `None` so the merge falls back to the diff-guess.
pub fn editor_ops_for_base(doc: &Path, base_text: &str) -> Result<Option<Vec<EditorOp>>> {
    let Some(sidecar) = load_op_capture(doc)? else {
        return Ok(None);
    };
    if sidecar.ops.is_empty() {
        return Ok(None);
    }
    if sidecar.base_hash == content_hash(base_text) {
        Ok(Some(sidecar.ops))
    } else {
        Ok(None)
    }
}

/// Clear the op-capture sidecar for `doc`. Idempotent — clearing an absent
/// sidecar succeeds.
pub fn clear_op_capture(doc: &Path) -> Result<()> {
    let path = op_capture_path_for(doc)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("failed to clear {}", path.display())),
    }
}

/// Garbage-collect op-capture sidecars under `project_root` whose `updated_ms`
/// is older than `max_age_secs`. Returns the number removed. A sidecar with no
/// usable timestamp (0 / unparseable) is also removed, since it cannot be a
/// fresh in-flight epoch. Best-effort: individual failures are logged, not
/// propagated.
pub fn gc_op_captures(project_root: &Path, max_age_secs: u64) -> Result<usize> {
    let dir = project_root.join(OP_CAPTURE_SUBDIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", dir.display())),
    };
    let cutoff_ms = now_millis().saturating_sub((max_age_secs as u128) * 1000);
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let stale = match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str::<OpCaptureSidecar>(&json)
                .map(|s| s.updated_ms == 0 || s.updated_ms < cutoff_ms)
                .unwrap_or(true),
            Err(_) => true,
        };
        if stale {
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(e) => eprintln!("[op-capture] gc: failed to remove {} ({e})", path.display()),
            }
        }
    }
    Ok(removed)
}

fn write_sidecar(doc: &Path, sidecar: &OpCaptureSidecar) -> Result<()> {
    let path = op_capture_path_for(doc)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(sidecar)?;
    atomic_write(&path, &json)
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    tmp.persist(path)
        .with_context(|| format!("failed to persist {}", path.display()))?;
    Ok(())
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_doc() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();
        (dir, doc)
    }

    #[test]
    fn record_then_load_roundtrip() {
        let (_dir, doc) = setup_doc();
        let base = "hello world\n";
        let h = content_hash(base);
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 5,
                text: "!".to_string(),
            },
        )
        .unwrap();
        let sidecar = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(sidecar.base_hash, h);
        assert_eq!(sidecar.ops.len(), 1);
    }

    #[test]
    fn record_appends_within_same_epoch() {
        let (_dir, doc) = setup_doc();
        let h = content_hash("base\n");
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 0,
                text: "a".into(),
            },
        )
        .unwrap();
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 1,
                text: "b".into(),
            },
        )
        .unwrap();
        let sidecar = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(sidecar.ops.len(), 2);
    }

    #[test]
    fn record_against_new_base_resets_epoch() {
        let (_dir, doc) = setup_doc();
        let h1 = content_hash("base one\n");
        record_editor_op(
            &doc,
            &h1,
            EditorOp::Insert {
                offset: 0,
                text: "x".into(),
            },
        )
        .unwrap();
        let h2 = content_hash("base two\n");
        record_editor_op(
            &doc,
            &h2,
            EditorOp::Insert {
                offset: 0,
                text: "y".into(),
            },
        )
        .unwrap();
        let sidecar = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(sidecar.base_hash, h2);
        assert_eq!(sidecar.ops.len(), 1, "new base must start a fresh epoch");
    }

    #[test]
    fn editor_ops_for_base_matches_only_on_hash() {
        let (_dir, doc) = setup_doc();
        let base = "the base text\n";
        let h = content_hash(base);
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 0,
                text: "z".into(),
            },
        )
        .unwrap();
        assert!(editor_ops_for_base(&doc, base).unwrap().is_some());
        assert!(
            editor_ops_for_base(&doc, "a different base\n")
                .unwrap()
                .is_none(),
            "ops captured against a different base must not be trusted"
        );
    }

    #[test]
    fn editor_ops_for_base_none_when_empty_or_absent() {
        let (_dir, doc) = setup_doc();
        assert!(editor_ops_for_base(&doc, "anything").unwrap().is_none());
    }

    #[test]
    fn clear_is_idempotent() {
        let (_dir, doc) = setup_doc();
        clear_op_capture(&doc).unwrap();
        let h = content_hash("b\n");
        record_editor_op(&doc, &h, EditorOp::Delete { offset: 0, len: 1 }).unwrap();
        assert!(load_op_capture(&doc).unwrap().is_some());
        clear_op_capture(&doc).unwrap();
        assert!(load_op_capture(&doc).unwrap().is_none());
        clear_op_capture(&doc).unwrap();
    }

    #[test]
    fn malformed_sidecar_treated_as_absent() {
        let (_dir, doc) = setup_doc();
        let path = op_capture_path_for(&doc).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not valid json").unwrap();
        assert!(load_op_capture(&doc).unwrap().is_none());
    }

    #[test]
    fn gc_removes_stale_and_zero_timestamp_sidecars() {
        let (dir, doc) = setup_doc();
        // Fresh epoch (current timestamp) — should survive a generous max_age.
        let h = content_hash("b\n");
        record_editor_op(
            &doc,
            &h,
            EditorOp::Insert {
                offset: 0,
                text: "a".into(),
            },
        )
        .unwrap();
        let removed = gc_op_captures(dir.path(), 3600).unwrap();
        assert_eq!(removed, 0, "fresh sidecar must not be GC'd");
        // Force a zero timestamp (no usable epoch) — should be reaped.
        let mut sidecar = load_op_capture(&doc).unwrap().unwrap();
        sidecar.updated_ms = 0;
        write_sidecar(&doc, &sidecar).unwrap();
        let removed = gc_op_captures(dir.path(), 3600).unwrap();
        assert_eq!(removed, 1);
        assert!(load_op_capture(&doc).unwrap().is_none());
    }

    #[test]
    fn gc_missing_dir_is_zero() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(gc_op_captures(dir.path(), 60).unwrap(), 0);
    }
}

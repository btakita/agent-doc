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
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

const OP_CAPTURE_SUBDIR: &str = ".agent-doc/op-capture";

/// Cheap (len, mtime-nanos) fingerprint of the two files that fully determine
/// `current_base_hash` (the committed snapshot and the overlay CRDT sidecar).
/// `None` when the file is absent — an absent file is itself a stable state, so
/// it caches the empty-text hash until one of them appears.
type BaseFingerprint = (Option<(u64, u128)>, Option<(u64, u128)>);

fn file_fingerprint(path: &Path) -> Option<(u64, u128)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

/// Per-document memo of the last `current_base_hash` result, keyed by the
/// snapshot + overlay fingerprint that produced it (`#qbasehashmemo`). The base
/// hash is a pure function of those two files, and neither changes during a
/// typing burst — without this memo every keystroke rebuilt the full-document
/// CRDT merge base (`CrdtDoc::from_text` over the whole doc) just to recompute a
/// constant, making large documents progressively harder to type in.
#[allow(clippy::type_complexity)]
fn base_hash_cache() -> &'static Mutex<HashMap<PathBuf, (BaseFingerprint, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (BaseFingerprint, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Count of full (cache-miss) base-hash recomputations, for memoization tests.
pub(crate) static BASE_HASH_RECOMPUTES: AtomicU64 = AtomicU64::new(0);

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

/// True when there are **pending** captured editor ops for `doc` — live
/// keystrokes the editor reported that a merge has not yet consumed+cleared.
///
/// This is the liveness anchor the CRDT merge base uses to tell a **live
/// deletion** (the overlay shrank because the user just deleted a line; the ops
/// are still pending) apart from a **genuinely stale** overlay (an older
/// committed subset; no pending ops). The lineage-free `from_markdown` overlay
/// leaves no causal history to compare, so the two are content-identical
/// (overlay ⊆ baseline) — only the still-uncleared op-capture sidecar marks the
/// divergence as live. After a committed cycle `merge_contents_crdt_with_ops`
/// clears the sidecar, so a truly stale overlay reads `false` here and still GCs.
///
/// Over-reporting is the safe direction: a lingering non-empty sidecar at worst
/// *preserves* a stale overlay (delayed GC), never discards a live edit.
pub fn has_pending_editor_ops(doc: &Path) -> bool {
    matches!(load_op_capture(doc), Ok(Some(sidecar)) if !sidecar.ops.is_empty())
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

/// Compute the base hash the editor reporters must stamp on captured ops so the
/// merge will accept them (`#qnodemerge4wire`).
///
/// This MUST return `content_hash` of the **exact same base text**
/// [`crate::merge::merge_contents_crdt_with_ops`] resolves at write time — the
/// persisted CRDT merge base decoded to text — otherwise the merge's
/// [`editor_ops_for_base`] gate rejects every op (`sidecar.base_hash !=
/// content_hash(base_text)`) and the feature silently no-ops. The base both
/// sides diverge from is the committed snapshot content (the same content
/// preflight saves as the next write's baseline at a synced point), so we mirror
/// the merge: snapshot → `crdt_merge_base_state` → `CrdtDoc::decode_state` →
/// `to_text` → `content_hash`. With no snapshot/CRDT state yet this is the
/// empty-text hash, matching the merge's `None => String::new()` base.
pub fn current_base_hash(doc: &Path) -> Result<String> {
    let snapshot_path = crate::snapshot::path_for(doc)?;
    let overlay_path = crate::snapshot::overlay_crdt_path_for(doc)?;

    // The base hash depends only on the snapshot + overlay contents, and neither
    // changes while the user is typing. Memoize on their fingerprints so a typing
    // burst is an O(1) cache hit instead of a full-document CRDT round-trip per
    // keystroke (`#qbasehashmemo`). A stale fingerprint only ever degrades to a
    // recompute — never a wrong hash — and a wrong hash would itself just fall
    // back to the diff-guess merge, so this is safe under any mtime granularity.
    let fingerprint: BaseFingerprint = (
        file_fingerprint(&snapshot_path),
        file_fingerprint(&overlay_path),
    );
    let cache_key = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());

    if let Ok(cache) = base_hash_cache().lock()
        && let Some((cached_fp, cached_hash)) = cache.get(&cache_key)
        && *cached_fp == fingerprint
    {
        return Ok(cached_hash.clone());
    }

    BASE_HASH_RECOMPUTES.fetch_add(1, Ordering::Relaxed);
    let baseline = read_optional_text(&snapshot_path)?.unwrap_or_default();
    let base = crate::snapshot::crdt_merge_base_state(doc, &baseline)?;
    let base_text = agent_doc_core::crdt::CrdtDoc::decode_state(&base.state)
        .map(|d| d.to_text())
        .unwrap_or_default();
    let hash = content_hash(&base_text);

    if let Ok(mut cache) = base_hash_cache().lock() {
        cache.insert(cache_key, (fingerprint, hash.clone()));
    }
    Ok(hash)
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
    fn current_base_hash_matches_merge_gate_so_stamped_ops_are_accepted() {
        // `#qnodemerge4wire` keystone invariant: ops the editor reporters stamp
        // with `current_base_hash()` MUST pass the write-time merge's
        // `editor_ops_for_base` gate (`base_hash == content_hash(base_text)`),
        // otherwise every capture is silently rejected and the feature no-ops.
        let (_dir, doc) = setup_doc();
        crate::snapshot::save(&doc, "# base\n\n## section\n").unwrap();

        let base_hash = current_base_hash(&doc).unwrap();
        record_editor_op(
            &doc,
            &base_hash,
            EditorOp::Insert {
                offset: 0,
                text: "x".into(),
            },
        )
        .unwrap();

        // Resolve the SAME base text `merge::merge_contents_crdt_with_ops` resolves.
        let snapshot = crate::snapshot::path_for(&doc).unwrap();
        let baseline = read_optional_text(&snapshot).unwrap().unwrap_or_default();
        let base = crate::snapshot::crdt_merge_base_state(&doc, &baseline).unwrap();
        let base_text = agent_doc_core::crdt::CrdtDoc::decode_state(&base.state)
            .map(|d| d.to_text())
            .unwrap_or_default();

        assert_eq!(
            base_hash,
            content_hash(&base_text),
            "current_base_hash must equal the merge's base_text hash"
        );
        let ops = editor_ops_for_base(&doc, &base_text).unwrap();
        assert!(
            ops.is_some(),
            "ops stamped with current_base_hash must pass the merge gate"
        );
        assert_eq!(ops.unwrap().len(), 1);
    }

    #[test]
    fn current_base_hash_is_empty_text_hash_without_snapshot() {
        // No snapshot/CRDT base yet → empty-text hash, matching the merge's
        // `None => String::new()` base so a first-edit capture still aligns.
        let (_dir, doc) = setup_doc();
        assert_eq!(current_base_hash(&doc).unwrap(), content_hash(""));
    }

    #[test]
    fn current_base_hash_is_memoized_until_base_changes() {
        // `#qbasehashmemo`: a typing burst leaves the snapshot + overlay
        // unchanged, so repeated `current_base_hash` calls (one per keystroke)
        // must be served from the memo without rebuilding the full-document CRDT
        // merge base. Process-isolated under nextest, so the global recompute
        // counter measures exactly this test's calls.
        let (_dir, doc) = setup_doc();
        crate::snapshot::save(&doc, "# base\n\n## section\n").unwrap();

        let before = BASE_HASH_RECOMPUTES.load(Ordering::Relaxed);
        let h1 = current_base_hash(&doc).unwrap();
        let after_first = BASE_HASH_RECOMPUTES.load(Ordering::Relaxed);
        assert_eq!(after_first, before + 1, "first call must recompute once");

        // Simulate a typing burst: many calls against the same unchanged base.
        for _ in 0..50 {
            assert_eq!(current_base_hash(&doc).unwrap(), h1);
        }
        assert_eq!(
            BASE_HASH_RECOMPUTES.load(Ordering::Relaxed),
            after_first,
            "repeated calls against an unchanged base must be cache hits"
        );

        // Changing the base (a real write boundary) must invalidate the memo.
        crate::snapshot::save(&doc, "# base\n\n## section\n\nmore\n").unwrap();
        let h2 = current_base_hash(&doc).unwrap();
        assert_ne!(h1, h2, "a changed snapshot must yield a fresh base hash");
        assert!(
            BASE_HASH_RECOMPUTES.load(Ordering::Relaxed) > after_first,
            "a changed base must trigger a recompute"
        );
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

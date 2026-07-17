//! # Module: op_capture_io (`#qnodemerge4wire`)
//!
//! Supply side of the op-capture / evented-reflection merge (`#qnodemerge4`).
//! The *consumer* (`agent_doc_merge::crdt::merge_with_editor_ops` + `EditorOp` /
//! `replay_editor_ops`) already trusts the editor's *real* operations over a
//! Myers diff-guess when those ops replay onto the merge base exactly. This
//! module is the durable plumbing that gets those ops from the editor to the
//! merge through the project's single `state.db` ledger:
//!
//! - **Persistence** — one `editor_op_captures` row holds the ordered ops the
//!   editor reported, plus the `base_hash` of the buffer text they were captured
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
//! - `clear_op_capture` is idempotent — clearing an absent row is `Ok(())`.
//! - `agent_doc_hash::content_hash` is the single source of the base-hash
//!   function shared by the producer (editor, via FFI) and the consumer (merge).
//!
//! Wiring point (`#qnodemerge4wire` part 3): `merge::merge_contents_crdt_with_ops`
//! loads ops via `editor_ops_for_base`, passes them to `merge_with_editor_ops`,
//! and clears the row after the merge. No per-document file is on this hot path.

use agent_doc_hash::content_hash;
use agent_doc_merge::crdt::EditorOp;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Per-document memo of the last `current_base_hash` result, keyed by the
/// durable-baseline content hash that produced it (`#qbasehashmemo`). No
/// filesystem projection participates in the typing hot path.
fn base_hash_cache() -> &'static Mutex<HashMap<PathBuf, (String, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (String, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Count of full (cache-miss) base-hash recomputations, for memoization tests.
pub(crate) static BASE_HASH_RECOMPUTES: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn base_hash_recomputes_by_doc_for_tests() -> &'static Mutex<HashMap<PathBuf, u64>> {
    static COUNTS: OnceLock<Mutex<HashMap<PathBuf, u64>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn record_base_hash_recompute_for_tests(doc: &Path) {
    if let Ok(mut counts) = base_hash_recomputes_by_doc_for_tests().lock() {
        *counts.entry(doc.to_path_buf()).or_insert(0) += 1;
    }
}

#[cfg(test)]
fn base_hash_recomputes_for_doc_for_tests(doc: &Path) -> u64 {
    let key = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    base_hash_recomputes_by_doc_for_tests()
        .lock()
        .ok()
        .and_then(|counts| counts.get(&key).copied())
        .unwrap_or(0)
}

/// The per-document operation epoch stored in `state.db`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpCaptureState {
    /// SHA256 hex of the buffer text the ops were captured against — the merge
    /// base they replay onto. The merge trusts the ops only when its resolved
    /// base hashes to this value.
    pub base_hash: String,
    /// Ordered editor operations, in the sequence the editor performed them.
    pub ops: Vec<EditorOp>,
    /// Last-update wall-clock (ms since epoch), best-effort, for GC.
    pub updated_ms: u64,
}

fn state_db_identity(doc: &Path) -> Result<(PathBuf, String, String)> {
    let canonical = doc.canonicalize()?;
    let hash = agent_doc_fs::document_state_hash(&canonical)?;
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    Ok((project_root, hash, canonical.to_string_lossy().into_owned()))
}

/// Load the operation epoch for `doc`, if present.
///
/// A malformed row is treated as absent (logged) rather than failing the
/// merge — a corrupt capture must never block a write.
pub fn load_op_capture(doc: &Path) -> Result<Option<OpCaptureState>> {
    let (project_root, document_hash, _) = state_db_identity(doc)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let Some(record) =
        agent_doc_sqlite::state_store::load_editor_op_capture_from_db(&conn, &document_hash)?
    else {
        return Ok(None);
    };
    match serde_json::from_str::<Vec<EditorOp>>(&record.ops_json) {
        Ok(ops) => Ok(Some(OpCaptureState {
            base_hash: record.base_hash,
            ops,
            updated_ms: record.updated_at_ms,
        })),
        Err(e) => {
            eprintln!(
                "[op-capture] ignoring malformed state row document_hash={} ({e}); falling back to diff-guess",
                document_hash,
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
/// (overlay ⊆ baseline) — only the still-uncleared op-capture row marks the
/// divergence as live. After a committed cycle `merge_contents_crdt_with_ops`
/// clears the row, so a truly stale overlay reads `false` here and still GCs.
///
/// Over-reporting is the safe direction: a lingering non-empty row at worst
/// *preserves* a stale overlay (delayed GC), never discards a live edit.
pub fn has_pending_editor_ops(doc: &Path) -> bool {
    matches!(load_op_capture(doc), Ok(Some(capture)) if !capture.ops.is_empty())
}

/// Record one editor op for `doc`, captured against the buffer whose text hashes
/// to `base_hash`.
///
/// Appends within the current epoch when `base_hash` matches the stored one;
/// otherwise starts a fresh epoch (the prior ops were captured against a base
/// that no longer applies, so they are discarded).
pub fn record_editor_op(doc: &Path, base_hash: &str, op: EditorOp) -> Result<()> {
    let (project_root, document_hash, canonical_path) = state_db_identity(doc)?;
    let mut conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let tx = conn.transaction()?;
    let existing =
        agent_doc_sqlite::state_store::load_editor_op_capture_from_db(&tx, &document_hash)?;
    let mut state = match existing {
        Some(existing) if existing.base_hash == base_hash => OpCaptureState {
            base_hash: existing.base_hash,
            ops: serde_json::from_str(&existing.ops_json).unwrap_or_default(),
            updated_ms: existing.updated_at_ms,
        },
        _ => OpCaptureState {
            base_hash: base_hash.to_string(),
            ops: Vec::new(),
            updated_ms: 0,
        },
    };
    state.ops.push(op);
    state.updated_ms = now_millis();
    agent_doc_sqlite::state_store::upsert_editor_op_capture_in_db(
        &tx,
        &agent_doc_sqlite::state_store::EditorOpCaptureRecord {
            document_hash,
            canonical_path,
            base_hash: state.base_hash,
            ops_json: serde_json::to_string(&state.ops)?,
            updated_at_ms: state.updated_ms,
        },
    )?;
    tx.commit()?;
    Ok(())
}

/// Compute the base hash the editor reporters must stamp on captured ops so the
/// merge will accept them (`#qnodemerge4wire`).
///
/// This MUST return `content_hash` of the **exact same base text**
/// `merge_contents_crdt_with_ops` resolves at write time — the
/// persisted CRDT merge base decoded to text — otherwise the merge's
/// [`editor_ops_for_base`] gate rejects every op (`capture.base_hash !=
/// content_hash(base_text)`) and the feature silently no-ops. The base both
/// sides diverge from is the committed snapshot content (the same content
/// preflight saves as the next write's baseline at a synced point), so the caller
/// supplies the same CRDT merge-base resolver the write path uses. With no
/// snapshot/CRDT state yet this is the empty-text hash, matching the merge's
/// `None => String::new()` base.
pub fn current_base_hash_with<R>(doc: &Path, resolve_base_state: R) -> Result<String>
where
    R: FnOnce(&Path, &str) -> Result<Vec<u8>>,
{
    let baseline = agent_doc_snapshot_io::load_document_baseline(doc)?.unwrap_or_default();
    let fingerprint = content_hash(&baseline);
    let cache_key = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());

    if let Ok(cache) = base_hash_cache().lock()
        && let Some((cached_fp, cached_hash)) = cache.get(&cache_key)
        && *cached_fp == fingerprint
    {
        return Ok(cached_hash.clone());
    }

    BASE_HASH_RECOMPUTES.fetch_add(1, Ordering::Relaxed);
    #[cfg(test)]
    record_base_hash_recompute_for_tests(&cache_key);
    let base_state = resolve_base_state(doc, &baseline)?;
    let base_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&base_state)
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
    let Some(capture) = load_op_capture(doc)? else {
        return Ok(None);
    };
    if capture.ops.is_empty() {
        return Ok(None);
    }
    if capture.base_hash == content_hash(base_text) {
        Ok(Some(capture.ops))
    } else {
        Ok(None)
    }
}

/// Clear the op-capture row for `doc`. Idempotent — clearing an absent row succeeds.
pub fn clear_op_capture(doc: &Path) -> Result<()> {
    let (project_root, document_hash, _) = state_db_identity(doc)?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    agent_doc_sqlite::state_store::clear_editor_op_capture_in_db(&conn, &document_hash)?;
    Ok(())
}

/// Garbage-collect op-capture rows whose `updated_ms` is older than
/// `max_age_secs`. Returns the number removed. A row with no usable timestamp
/// is also removed, since it cannot be a
/// fresh in-flight epoch. Best-effort: individual failures are logged, not
/// propagated.
pub fn gc_op_captures(project_root: &Path, max_age_secs: u64) -> Result<usize> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let cutoff_ms = now_millis().saturating_sub(max_age_secs.saturating_mul(1000));
    agent_doc_sqlite::state_store::gc_editor_op_captures_in_db(&conn, cutoff_ms)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
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
        let base_text = "# base\n\n## section\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base_text).encode_state();

        let base_hash =
            current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state.clone())).unwrap();
        record_editor_op(
            &doc,
            &base_hash,
            EditorOp::Insert {
                offset: 0,
                text: "x".into(),
            },
        )
        .unwrap();

        // Resolve the SAME base text `merge_contents_crdt_with_ops` resolves.
        let base_text = agent_doc_merge::crdt::CrdtDoc::decode_state(&base_state)
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
        assert_eq!(
            current_base_hash_with(&doc, |_doc, _baseline| {
                Ok(agent_doc_merge::crdt::CrdtDoc::from_text("").encode_state())
            })
            .unwrap(),
            content_hash("")
        );
    }

    #[test]
    fn current_base_hash_is_memoized_until_base_changes() {
        // `#qbasehashmemo`: a typing burst leaves the snapshot + overlay
        // unchanged, so repeated `current_base_hash` calls (one per keystroke)
        // must be served from the memo without rebuilding the full-document CRDT
        // merge base. The per-document recompute counter avoids parallel-test
        // noise from other temp documents.
        let (_dir, doc) = setup_doc();
        let base_text = "# base\n\n## section\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base_text).encode_state();

        let before = base_hash_recomputes_for_doc_for_tests(&doc);
        let h1 = current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state.clone())).unwrap();
        let after_first = base_hash_recomputes_for_doc_for_tests(&doc);
        assert_eq!(after_first, before + 1, "first call must recompute once");

        // Simulate a typing burst: many calls against the same unchanged base.
        for _ in 0..50 {
            assert_eq!(
                current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state.clone())).unwrap(),
                h1
            );
        }
        assert_eq!(
            base_hash_recomputes_for_doc_for_tests(&doc),
            after_first,
            "repeated calls against an unchanged base must be cache hits"
        );

        // Changing the base (a real write boundary) must invalidate the memo.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "# base\n\n## section\n\nmore\n",
            |_, _| {},
        )
        .unwrap();
        let base_state_2 =
            agent_doc_merge::crdt::CrdtDoc::from_text("# base\n\n## section\n\nmore\n")
                .encode_state();
        let h2 = current_base_hash_with(&doc, |_doc, _baseline| Ok(base_state_2.clone())).unwrap();
        assert_ne!(h1, h2, "a changed snapshot must yield a fresh base hash");
        assert!(
            base_hash_recomputes_for_doc_for_tests(&doc) > after_first,
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
        let capture = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(capture.base_hash, h);
        assert_eq!(capture.ops.len(), 1);
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
        let capture = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(capture.ops.len(), 2);
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
        let capture = load_op_capture(&doc).unwrap().unwrap();
        assert_eq!(capture.base_hash, h2);
        assert_eq!(capture.ops.len(), 1, "new base must start a fresh epoch");
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
    fn malformed_state_row_treated_as_absent() {
        let (_dir, doc) = setup_doc();
        let (project_root, document_hash, canonical_path) = state_db_identity(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&project_root).unwrap();
        agent_doc_sqlite::state_store::upsert_editor_op_capture_in_db(
            &conn,
            &agent_doc_sqlite::state_store::EditorOpCaptureRecord {
                document_hash,
                canonical_path,
                base_hash: "base".to_string(),
                ops_json: "{not valid json".to_string(),
                updated_at_ms: now_millis(),
            },
        )
        .unwrap();
        assert!(load_op_capture(&doc).unwrap().is_none());
    }

    #[test]
    fn gc_removes_stale_and_zero_timestamp_rows() {
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
        assert_eq!(removed, 0, "fresh row must not be GC'd");
        // Force a zero timestamp (no usable epoch) — should be reaped.
        let capture = load_op_capture(&doc).unwrap().unwrap();
        let (project_root, document_hash, canonical_path) = state_db_identity(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&project_root).unwrap();
        agent_doc_sqlite::state_store::upsert_editor_op_capture_in_db(
            &conn,
            &agent_doc_sqlite::state_store::EditorOpCaptureRecord {
                document_hash,
                canonical_path,
                base_hash: capture.base_hash,
                ops_json: serde_json::to_string(&capture.ops).unwrap(),
                updated_at_ms: 0,
            },
        )
        .unwrap();
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

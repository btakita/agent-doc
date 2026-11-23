//! Revision-keyed memo for repeated authority-hash observations
//! (dogfood `maintlockhold`).
//!
//! ## Spec
//! - Controller-owned document maintenance crosses its effect fence dozens to
//!   hundreds of times per run, and every crossing used to materialize the
//!   whole canonical document under the relay hub lock just to hash it. On a
//!   963KB document that made the maintenance claim last 42-185 s for ~2 s of
//!   compute, stalled every other hub reader (editor snapshot handling went
//!   silent for the same window), and failed operator mutations waiting on the
//!   claim (haiven-dev sdk-dev.md, 2026-11-23).
//! - A fence only needs the authority *hash*. The relay hub exposes a compact
//!   revision (lineage, state vector, text revision) without materializing
//!   text. While that revision is unchanged the canonical text is unchanged,
//!   so the previous hash is reused and no text is materialized.
//! - The memo caches a hash only when the compact revision read before and
//!   after the materialization agree, so a change that lands mid-read is never
//!   cached under a stale key.
//!
//! ## Agentic Contracts
//! - Without a compact revision (no in-process hub, a detached or
//!   missing-replica authority, a controller that predates `text_revision`)
//!   every observation materializes exactly as before.
//! - The memo never reads disk, `agent:exchange`, or any other bytes beyond
//!   what one uncached observation reads.
//!
//! ## Evals
//! - `an_unchanged_revision_materializes_once`
//! - `a_revision_change_rematerializes`
//! - `no_revision_always_materializes`
//! - `a_change_during_materialization_is_never_cached`

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;

/// The compact identity of the live canonical text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityRevisionKey {
    lineage: String,
    state_vector: Vec<u8>,
    text_revision: u64,
}

/// Observation counters for one memo (diagnostics and tests).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuthorityHashMemoCounts {
    pub observations: u64,
    pub materializations: u64,
}

#[derive(Default)]
struct MemoState {
    cached: Option<(AuthorityRevisionKey, String)>,
    counts: AuthorityHashMemoCounts,
}

/// Memoizes `content_hash(peek_current_document_content(file))` by the live
/// relay's compact revision. One memo serves one document.
#[derive(Default)]
pub struct AuthorityHashMemo {
    state: Mutex<MemoState>,
}

impl AuthorityHashMemo {
    pub fn new() -> Self {
        Self::default()
    }

    /// The current authority hash of `file`, as
    /// [`crate::peek_current_document_content`] would hash it.
    pub fn observe_hash(&self, file: &Path, source: &str) -> Result<String> {
        self.observe_hash_with(
            || compact_authority_revision_key(file),
            || {
                crate::peek_current_document_content(file, source)
                    .map(|content| agent_doc_hash::content_hash(&content))
            },
        )
    }

    pub fn counts(&self) -> AuthorityHashMemoCounts {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .counts
    }

    fn observe_hash_with(
        &self,
        mut revision: impl FnMut() -> Option<AuthorityRevisionKey>,
        materialize: impl FnOnce() -> Result<String>,
    ) -> Result<String> {
        let before = revision();
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.counts.observations += 1;
            if let (Some(key), Some((cached_key, hash))) = (&before, &state.cached)
                && key == cached_key
            {
                return Ok(hash.clone());
            }
        }
        let hash = materialize()?;
        let after = revision();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.counts.materializations += 1;
        state.cached = match (before, after) {
            (Some(before), Some(after)) if before == after => Some((after, hash.clone())),
            _ => None,
        };
        Ok(hash)
    }
}

/// The in-process relay hub's compact revision of `file`'s canonical text, or
/// `None` when this process has no hub that answers the peek for it.
fn compact_authority_revision_key(file: &Path) -> Option<AuthorityRevisionKey> {
    if !agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        return None;
    }
    let authority = agent_doc_crdt_relay_io::crdt_authority_for_file(file);
    match agent_doc_crdt_relay_io::current_revision_for_file_with_authority(file, authority).ok()? {
        agent_doc_crdt_relay_io::CurrentRevision::Current {
            lineage,
            state_vector,
            text_revision,
            ..
        } if text_revision != 0 => Some(AuthorityRevisionKey {
            lineage,
            state_vector,
            text_revision,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn key(revision: u64) -> AuthorityRevisionKey {
        AuthorityRevisionKey {
            lineage: "lineage".to_string(),
            state_vector: vec![1, 2, 3],
            text_revision: revision,
        }
    }

    #[test]
    fn an_unchanged_revision_materializes_once() {
        let memo = AuthorityHashMemo::new();
        let materialized = Cell::new(0);
        for _ in 0..200 {
            let hash = memo
                .observe_hash_with(
                    || Some(key(7)),
                    || {
                        materialized.set(materialized.get() + 1);
                        Ok("hash-7".to_string())
                    },
                )
                .unwrap();
            assert_eq!(hash, "hash-7");
        }
        assert_eq!(materialized.get(), 1);
        assert_eq!(
            memo.counts(),
            AuthorityHashMemoCounts {
                observations: 200,
                materializations: 1
            }
        );
    }

    #[test]
    fn a_revision_change_rematerializes() {
        let memo = AuthorityHashMemo::new();
        let first = memo
            .observe_hash_with(|| Some(key(1)), || Ok("hash-1".to_string()))
            .unwrap();
        let second = memo
            .observe_hash_with(|| Some(key(2)), || Ok("hash-2".to_string()))
            .unwrap();
        assert_eq!((first.as_str(), second.as_str()), ("hash-1", "hash-2"));
        let mut lineage_change = key(2);
        lineage_change.lineage = "replaced".to_string();
        let third = memo
            .observe_hash_with(|| Some(lineage_change.clone()), || Ok("hash-3".to_string()))
            .unwrap();
        assert_eq!(third, "hash-3");
        assert_eq!(memo.counts().materializations, 3);
    }

    #[test]
    fn no_revision_always_materializes() {
        let memo = AuthorityHashMemo::new();
        for round in 0..3 {
            let hash = memo
                .observe_hash_with(|| None, || Ok(format!("disk-{round}")))
                .unwrap();
            assert_eq!(hash, format!("disk-{round}"));
        }
        assert_eq!(memo.counts().materializations, 3);
    }

    #[test]
    fn a_change_during_materialization_is_never_cached() {
        let memo = AuthorityHashMemo::new();
        let reads = Cell::new(0u64);
        // Revision 1 before the read, 2 after it: the hash may describe
        // either text, so it must not be cached under either key.
        memo.observe_hash_with(
            || {
                reads.set(reads.get() + 1);
                Some(key(reads.get()))
            },
            || Ok("racy".to_string()),
        )
        .unwrap();
        let hash = memo
            .observe_hash_with(|| Some(key(2)), || Ok("hash-2".to_string()))
            .unwrap();
        assert_eq!(hash, "hash-2");
        assert_eq!(memo.counts().materializations, 2);
    }
}

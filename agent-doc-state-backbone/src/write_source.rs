//! Typed write-intent discriminant (`#adwritesourceenum`).
//!
//! `DocumentWriteIntentProjection.source` used to be a `String`, and three
//! crates already keyed behavior off it by prefix and equality:
//!
//! - `agent-doc-document-realtime-io` gated the transient-prompt-marker retirement
//!   on `starts_with("serialized_atomic_write")` and the superseded-compact sweep
//!   on `== "post_commit_reposition" || starts_with("serialized_atomic_write")`;
//! - `agent-doc-write-ipc-io` routed the post-commit reposition transport on
//!   `starts_with("force_disk")`;
//! - `agent-doc-document-realtime-io` kept an external-disk candidate's base on
//!   both sides being `starts_with("force_disk")`.
//!
//! Nothing enumerated the values, so nothing could say what they *mean*. That is
//! why the 2026-07-26 retained-hash deadlock needed a content diff to answer a
//! question the type should have answered: a closeout writes the response cell,
//! then a `pending_write` carrying response+backlog, then the `pending_add_sync`
//! queue mirror, then `post_commit_reposition` — four **consecutive stages of one
//! closeout** — but nothing said so, so a retained intent could not ask "was I
//! superseded by my own successor" and had to infer it from added lines.
//!
//! [`CloseoutStage`] makes that an ordering comparison. Sources that are not a
//! closeout stage — the operator force-disk escape hatches, the generic
//! serialized transport tag, and every unrecognized value — answer `None` and are
//! never ordered against anything.
//!
//! # Unknown round-trips, it does not fail
//!
//! `source` is serialized into `state.db` projections, so a peer on an older or
//! newer build will read values this build does not know. [`DocumentWriteSource::Unknown`]
//! preserves the exact token through deserialize/serialize. A hard error here
//! would wedge precisely the cross-version case the type is meant to make safe.
//! This mirrors `DocumentWriteDeferredReason::Legacy`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The ordered stages of a single closeout.
///
/// Declaration order **is** the ordering: each stage writes a document the
/// previous stage produced, so a later stage's target is a superset of the
/// earlier one's. That is the whole point — supersession within one closeout
/// becomes `later > earlier` instead of a line-set diff.
///
/// Only sources that genuinely belong to that sequence map here. A tag that
/// merely describes *transport* (`serialized_atomic_write`) or an operator
/// escape hatch (`force_disk`) has no position in it, and must not be ordered
/// against one that does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseoutStage {
    /// The tracked-work write carrying the assistant response and backlog
    /// mutations (`converge_or_disk_write(.., "pending_write")`).
    ResponseWrite,
    /// The `agent:queue` mirror written seconds later, whose target is a
    /// superset of the response write's.
    QueueMirror,
    /// Boundary / `(HEAD)` marker cleanup once the commit has landed.
    PostCommitReposition,
}

impl CloseoutStage {
    pub const fn token(self) -> &'static str {
        match self {
            Self::ResponseWrite => "response_write",
            Self::QueueMirror => "queue_mirror",
            Self::PostCommitReposition => "post_commit_reposition",
        }
    }
}

impl fmt::Display for CloseoutStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.token())
    }
}

/// What produced a document write intent.
///
/// The JSON representation stays the same snake_case string the `String` field
/// held, so existing controller databases remain readable in both directions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DocumentWriteSource {
    /// Closeout stage 1: response + backlog tracked-work write.
    PendingWrite,
    /// Closeout stage 2: the `agent:queue` mirror for ids created this cycle.
    PendingAddSync,
    /// Closeout stage 2, taken through the tracked-work rewrite fallback.
    PendingAddSyncFallback,
    /// Closeout stage 3: boundary / `(HEAD)` marker cleanup after the commit.
    PostCommitReposition,
    /// Generic serialized whole-document write through the CRDT authority. This
    /// is a *transport* tag, not a closeout stage — any stage can arrive on it.
    SerializedAtomicWrite,
    /// The editor ACKed the canonical target but has not projected its own save.
    SerializedAtomicWriteEditorSavePending,
    /// Editor authority advanced after delivery proof; the target was rebased.
    SerializedAtomicWriteProjectionRebase,
    /// A reappearing editor buffer was merged with a retained canonical target.
    ///
    /// These snapshots are progressive editor cuts, not independent durable
    /// mutations. A later retained target that incorporates one supersedes it.
    EditorReconnect,
    /// Operator escape hatch: `agent-doc write --force-disk` on a detached document.
    ForceDisk,
    /// Force-disk taken from the repair path.
    RepairForceDisk,
    /// Any value this build does not recognize, preserved verbatim.
    Unknown(String),
}

impl DocumentWriteSource {
    pub fn token(&self) -> &str {
        match self {
            Self::PendingWrite => "pending_write",
            Self::PendingAddSync => "pending_add_sync",
            Self::PendingAddSyncFallback => "pending_add_sync_fallback",
            Self::PostCommitReposition => "post_commit_reposition",
            Self::SerializedAtomicWrite => "serialized_atomic_write",
            Self::SerializedAtomicWriteEditorSavePending => {
                "serialized_atomic_write_editor_save_pending"
            }
            Self::SerializedAtomicWriteProjectionRebase => {
                "serialized_atomic_write_projection_rebase"
            }
            Self::EditorReconnect => "editor_reconnect",
            Self::ForceDisk => "force_disk",
            Self::RepairForceDisk => "repair_force_disk",
            Self::Unknown(token) => token,
        }
    }

    /// Where this source sits in the closeout sequence, if it sits in it at all.
    ///
    /// `None` is a real answer, not a missing one: a transport tag or an
    /// operator escape hatch has no stage, and ordering it against one that does
    /// would invent a supersession relation that does not exist.
    pub fn closeout_stage(&self) -> Option<CloseoutStage> {
        match self {
            Self::PendingWrite => Some(CloseoutStage::ResponseWrite),
            Self::PendingAddSync | Self::PendingAddSyncFallback => Some(CloseoutStage::QueueMirror),
            Self::PostCommitReposition => Some(CloseoutStage::PostCommitReposition),
            Self::SerializedAtomicWrite
            | Self::SerializedAtomicWriteEditorSavePending
            | Self::SerializedAtomicWriteProjectionRebase
            | Self::EditorReconnect
            | Self::ForceDisk
            | Self::RepairForceDisk
            | Self::Unknown(_) => None,
        }
    }

    /// Is this the serialized whole-document transport, in any of its phases?
    ///
    /// Replaces `source.starts_with("serialized_atomic_write")`, which also
    /// matched any future tag that happened to share the prefix.
    pub fn is_serialized_atomic_write(&self) -> bool {
        matches!(
            self,
            Self::SerializedAtomicWrite
                | Self::SerializedAtomicWriteEditorSavePending
                | Self::SerializedAtomicWriteProjectionRebase
        )
    }

    /// Is this the operator-authorized force-disk projection whose retained
    /// target must be materialized through the force-disk transport?
    ///
    /// Replaces `source.starts_with("force_disk")`, and deliberately keeps that
    /// check's exact membership: `repair_force_disk` does **not** share the
    /// prefix and never matched, so it must not start matching now. The two are
    /// genuinely different intents — `force_disk` *is* the disk projection,
    /// while `repair_force_disk` retains the editor-reconnect lineage
    /// *before* one (`RetainEditorReconnectLineageBeforeDiskProjection`).
    pub fn is_force_disk(&self) -> bool {
        matches!(self, Self::ForceDisk)
    }

    /// Both force-disk shapes, for callers that mean the family rather than the
    /// transport decision. No current behavior site uses this — it exists so a
    /// future caller states which of the two it means instead of reaching for a
    /// prefix again.
    pub fn is_force_disk_family(&self) -> bool {
        matches!(self, Self::ForceDisk | Self::RepairForceDisk)
    }

    pub fn is_post_commit_reposition(&self) -> bool {
        matches!(self, Self::PostCommitReposition)
    }

    /// Did `later` supersede a target retained by `self` as the **next stage of
    /// the same closeout**?
    ///
    /// Both sides must carry a stage. An unstaged source on either side answers
    /// `false`: "I do not know where this sits" is not "it came after you".
    pub fn superseded_by(&self, later: &Self) -> bool {
        match (self.closeout_stage(), later.closeout_stage()) {
            (Some(mine), Some(theirs)) => theirs > mine,
            _ => false,
        }
    }
}

impl Default for DocumentWriteSource {
    fn default() -> Self {
        Self::Unknown(String::new())
    }
}

impl From<&str> for DocumentWriteSource {
    fn from(value: &str) -> Self {
        match value {
            "pending_write" => Self::PendingWrite,
            "pending_add_sync" => Self::PendingAddSync,
            "pending_add_sync_fallback" => Self::PendingAddSyncFallback,
            "post_commit_reposition" => Self::PostCommitReposition,
            "serialized_atomic_write" => Self::SerializedAtomicWrite,
            "serialized_atomic_write_editor_save_pending" => {
                Self::SerializedAtomicWriteEditorSavePending
            }
            "serialized_atomic_write_projection_rebase" => {
                Self::SerializedAtomicWriteProjectionRebase
            }
            "editor_reconnect" => Self::EditorReconnect,
            "force_disk" => Self::ForceDisk,
            "repair_force_disk" => Self::RepairForceDisk,
            token => Self::Unknown(token.to_string()),
        }
    }
}

impl From<String> for DocumentWriteSource {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl fmt::Display for DocumentWriteSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.token())
    }
}

impl PartialEq<&str> for DocumentWriteSource {
    fn eq(&self, other: &&str) -> bool {
        self.token() == *other
    }
}

impl Serialize for DocumentWriteSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.token())
    }
}

impl<'de> Deserialize<'de> for DocumentWriteSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tokens_round_trip_through_json() {
        for token in [
            "pending_write",
            "pending_add_sync",
            "pending_add_sync_fallback",
            "post_commit_reposition",
            "serialized_atomic_write",
            "serialized_atomic_write_editor_save_pending",
            "serialized_atomic_write_projection_rebase",
            "editor_reconnect",
            "force_disk",
            "repair_force_disk",
        ] {
            let parsed = DocumentWriteSource::from(token);
            assert!(
                !matches!(parsed, DocumentWriteSource::Unknown(_)),
                "{token} should be a recognized variant"
            );
            let json = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, format!("\"{token}\""));
            let back: DocumentWriteSource = serde_json::from_str(&json).unwrap();
            assert_eq!(back, parsed);
            assert_eq!(back.token(), token);
        }
    }

    /// The cross-version constraint: a peer writes a source this build has never
    /// heard of. Deserialization must succeed and re-serialize the exact token,
    /// because a hard error would wedge the very case the enum exists to make
    /// safe.
    #[test]
    fn unknown_token_round_trips_verbatim_instead_of_failing() {
        let json = "\"some_future_stage_from_a_newer_peer\"";
        let parsed: DocumentWriteSource = serde_json::from_str(json).unwrap();
        assert_eq!(
            parsed,
            DocumentWriteSource::Unknown("some_future_stage_from_a_newer_peer".to_string())
        );
        assert_eq!(serde_json::to_string(&parsed).unwrap(), json);
        assert_eq!(parsed.closeout_stage(), None);
        assert!(!parsed.is_force_disk());
        assert!(!parsed.is_serialized_atomic_write());
    }

    #[test]
    fn closeout_stages_are_ordered_response_then_mirror_then_reposition() {
        assert!(CloseoutStage::ResponseWrite < CloseoutStage::QueueMirror);
        assert!(CloseoutStage::QueueMirror < CloseoutStage::PostCommitReposition);
    }

    /// Stages are **optional**, which is why this is an ordering and not a state
    /// machine like `write_pipeline::DocumentWritePipeline`.
    ///
    /// That machine's central rule is `target.rank() == current.rank() + 1` —
    /// never backward, never skip — because a write intent must prove every
    /// delivery phase in turn. Closeout stages do not work that way: a cycle
    /// that created no backlog ids emits no `pending_add_sync` queue mirror at
    /// all, so `ResponseWrite → PostCommitReposition` is a normal closeout, not
    /// a skipped proof. Adding a no-skip transition relation here would reject
    /// the common case.
    ///
    /// Monotonicity is the only invariant, and `Ord` already is it.
    #[test]
    fn a_closeout_may_skip_a_stage_so_only_monotonicity_is_asserted() {
        assert!(
            DocumentWriteSource::PendingWrite
                .superseded_by(&DocumentWriteSource::PostCommitReposition),
            "a closeout with no queue mirror must still supersede its response write"
        );
    }

    /// The 2026-07-26 deadlock, as an ordering comparison: a retained
    /// `pending_write` target superseded by the `pending_add_sync` queue mirror
    /// of the same closeout.
    #[test]
    fn queue_mirror_supersedes_the_response_write_of_the_same_closeout() {
        assert!(
            DocumentWriteSource::PendingWrite.superseded_by(&DocumentWriteSource::PendingAddSync)
        );
        assert!(
            DocumentWriteSource::PendingWrite
                .superseded_by(&DocumentWriteSource::PendingAddSyncFallback)
        );
        assert!(
            DocumentWriteSource::PendingAddSync
                .superseded_by(&DocumentWriteSource::PostCommitReposition)
        );
    }

    #[test]
    fn supersession_is_strict_and_never_runs_backward() {
        assert!(
            !DocumentWriteSource::PendingAddSync.superseded_by(&DocumentWriteSource::PendingWrite)
        );
        assert!(
            !DocumentWriteSource::PendingWrite.superseded_by(&DocumentWriteSource::PendingWrite)
        );
    }

    /// An unstaged source must never participate in the ordering. Treating
    /// `force_disk` or an unknown token as "came after you" would settle a
    /// retained intent on no evidence at all.
    #[test]
    fn unstaged_sources_never_supersede_and_are_never_superseded() {
        let staged = DocumentWriteSource::PendingWrite;
        for unstaged in [
            DocumentWriteSource::ForceDisk,
            DocumentWriteSource::RepairForceDisk,
            DocumentWriteSource::SerializedAtomicWrite,
            DocumentWriteSource::Unknown("whatever".to_string()),
        ] {
            assert!(
                !staged.superseded_by(&unstaged),
                "{unstaged} must not supersede"
            );
            assert!(
                !unstaged.superseded_by(&staged),
                "{unstaged} must not be superseded"
            );
        }
    }

    #[test]
    fn typed_predicates_replace_the_string_prefix_checks() {
        assert!(DocumentWriteSource::SerializedAtomicWrite.is_serialized_atomic_write());
        assert!(
            DocumentWriteSource::SerializedAtomicWriteEditorSavePending
                .is_serialized_atomic_write()
        );
        assert!(
            DocumentWriteSource::SerializedAtomicWriteProjectionRebase.is_serialized_atomic_write()
        );
        assert!(!DocumentWriteSource::PostCommitReposition.is_serialized_atomic_write());
        assert!(DocumentWriteSource::ForceDisk.is_force_disk());
        assert!(!DocumentWriteSource::PendingWrite.is_force_disk());
        // Parity with the `starts_with("force_disk")` check this replaces:
        // `repair_force_disk` never shared the prefix, so it must not start
        // taking the force-disk transport branch now.
        assert!(!DocumentWriteSource::RepairForceDisk.is_force_disk());
        assert!(DocumentWriteSource::RepairForceDisk.is_force_disk_family());
        assert!(DocumentWriteSource::ForceDisk.is_force_disk_family());
        assert!(DocumentWriteSource::PostCommitReposition.is_post_commit_reposition());

        // The prefix checks these replace matched by string shape, so a source
        // that merely *starts with* a known tag was silently swept in. The enum
        // does not: an unrecognized token is `Unknown`, never a near-match.
        let lookalike = DocumentWriteSource::from("force_disk_projection_v2");
        assert!(!lookalike.is_force_disk());
        assert_eq!(
            lookalike,
            DocumentWriteSource::Unknown("force_disk_projection_v2".to_string())
        );
    }
}

//! `#netadv3` SIM-F1 / SIM-F2: send-order fencing for level-state updates.
//!
//! Supervisor lifecycle (`mark_lifecycle`), the supervisor heartbeat (which
//! carries the runtime state) and queue control (`queue_control`) each carry the
//! actor GENERATION and nothing else. The generation fences a straggler from a
//! previous owner, but within one generation the controller applied whatever
//! arrived LAST. Under delay, reorder or duplication a stale `Ready` could
//! overwrite a newer `Busy` (the next dispatch is typed into a busy pane), and a
//! stale pause/resume could flip queue control back. Found by the `#netadv4`
//! SimWorld network layer; modelled in `formal/tla/LifecycleSequence.tla`, whose
//! `LifecycleSequenceReorderWedge` is the generation-only fence.
//!
//! The fix stamps each update at SEND time and has the receiver discard one that
//! is older than the newest it applied for the same family, document and
//! generation. This module is the receiver's decision; the stamp source is a
//! per-host monotonic clock in the IO layer. A stamp is an ORDER, never a
//! duration: no timeout appears in the decision.

/// One level-state family whose updates are fenced independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SequencedFamily {
    /// `mark_lifecycle` actor state transitions.
    Lifecycle,
    /// `supervisor_heartbeat` runtime-state reports.
    Heartbeat,
    /// `queue_control` pause / resume / drain.
    QueueControl,
}

impl SequencedFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lifecycle => "lifecycle",
            Self::Heartbeat => "heartbeat",
            Self::QueueControl => "queue_control",
        }
    }
}

/// The newest stamp the receiver applied for one (family, scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceMark {
    pub generation: u64,
    pub stamp: u64,
}

/// What the receiver does with one stamped update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceAdmission {
    /// Apply it, and remember `next` as the newest mark.
    Apply { next: Option<SequenceMark> },
    /// It was sent before an update already applied in the same generation:
    /// discard it. Applying it would regress the receiver's view.
    Stale { newest: u64 },
}

/// Decide one stamped update.
///
/// * An unstamped update (an older client) is applied and leaves the mark
///   unchanged: it can neither be fenced nor fence a later one.
/// * A different generation is not ordered against the mark at all: the
///   generation fence (checked by the caller) owns that, and the new generation
///   starts a fresh mark.
/// * Within one generation an OLDER stamp is stale. An EQUAL stamp is the same
///   update again (a retry or a duplicate), applied idempotently.
pub fn admit_sequenced(
    mark: Option<SequenceMark>,
    generation: u64,
    stamp: Option<u64>,
) -> SequenceAdmission {
    let Some(stamp) = stamp else {
        return SequenceAdmission::Apply { next: mark };
    };
    match mark {
        Some(mark) if mark.generation == generation && stamp < mark.stamp => {
            SequenceAdmission::Stale { newest: mark.stamp }
        }
        Some(mark) if mark.generation == generation => SequenceAdmission::Apply {
            next: Some(SequenceMark {
                generation,
                stamp: stamp.max(mark.stamp),
            }),
        },
        _ => SequenceAdmission::Apply {
            next: Some(SequenceMark { generation, stamp }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(mark: Option<SequenceMark>, generation: u64, stamp: u64) -> Option<SequenceMark> {
        match admit_sequenced(mark, generation, Some(stamp)) {
            SequenceAdmission::Apply { next } => next,
            SequenceAdmission::Stale { .. } => panic!("expected apply"),
        }
    }

    /// SIM-F1 minimal trace: Ready (stamp 10) then Busy (stamp 20) are sent;
    /// the channel delivers Busy first. The late Ready is discarded instead of
    /// re-opening dispatch into a busy supervisor.
    #[test]
    fn a_reordered_older_update_in_the_same_generation_is_stale() {
        let mark = apply(None, 7, 20); // Busy arrives first
        assert_eq!(
            admit_sequenced(mark, 7, Some(10)), // the delayed Ready
            SequenceAdmission::Stale { newest: 20 }
        );
    }

    #[test]
    fn a_duplicate_of_the_newest_update_is_applied_idempotently() {
        let mark = apply(None, 7, 20);
        assert_eq!(apply(mark, 7, 20), mark);
        assert_eq!(apply(mark, 7, 30).unwrap().stamp, 30);
    }

    #[test]
    fn a_new_generation_starts_a_fresh_mark_even_with_a_smaller_stamp() {
        let mark = apply(None, 7, 20);
        assert_eq!(
            apply(mark, 8, 5),
            Some(SequenceMark {
                generation: 8,
                stamp: 5
            })
        );
    }

    #[test]
    fn an_unstamped_update_is_applied_and_neither_fences_nor_is_fenced() {
        let mark = apply(None, 7, 20);
        assert_eq!(
            admit_sequenced(mark, 7, None),
            SequenceAdmission::Apply { next: mark }
        );
        assert_eq!(
            admit_sequenced(None, 7, None),
            SequenceAdmission::Apply { next: None }
        );
    }
}

//! # Module: op_capture_refusal_coalesce
//!
//! ## Spec
//! - Coalesces repeated, identical `editor_op_capture_refused` receipts for one
//!   document (GH #242). A refusal whose `(reason, detail)` matches the last one
//!   emitted for the same path within [`REFUSAL_COALESCE_WINDOW`] is suppressed and
//!   counted instead of writing another `ops.log` line and making another Lazily
//!   actor round trip.
//! - The first refusal of a run is always emitted. A repeat after the window is
//!   emitted with the suppressed count (`coalesced_repeats=<n>`). A different
//!   refusal for the same path first flushes the prior run's suppressed count.
//! - Editor-agnostic: every editor reaches it through
//!   `agent_doc_log_editor_op_capture_refusal`, so a reporter loop in any editor
//!   cannot flood `ops.log` or the controller actor.
//!
//! ## Agentic Contracts
//! - Pure decision over an injected clock; no IO.
//! - The tracked-path set is bounded ([`MAX_TRACKED_PATHS`]); overflow resets the
//!   table, which can only cause an extra emission, never a lost first receipt.
//!
//! ## Evals
//! - `first_refusal_is_emitted_and_identical_repeats_are_counted`
//! - `a_repeat_after_the_window_carries_the_suppressed_count`
//! - `a_different_refusal_flushes_the_prior_run`

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long an identical refusal for one path is folded into a count.
///
/// Shorter than the controller's 10 s Compact Exchange quiescence window, so the
/// `doc_advanced_during_drain` activity refresh still keeps an actively-typing
/// document deferred.
pub(crate) const REFUSAL_COALESCE_WINDOW: Duration = Duration::from_secs(2);

/// Upper bound on documents tracked at once.
pub(crate) const MAX_TRACKED_PATHS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SuppressedRun {
    pub reason: String,
    pub detail: String,
    pub suppressed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefusalAdmission {
    /// Write the receipt (and run its side effects).
    Emit {
        /// Identical refusals folded since the last emission of this one.
        coalesced_repeats: u64,
        /// A different refusal's run that ended with unreported repeats.
        flushed: Option<SuppressedRun>,
    },
    /// Identical to the last emission inside the window: count it, write nothing.
    Suppress,
}

#[derive(Debug)]
struct Entry {
    reason: String,
    detail: String,
    emitted_at: Instant,
    suppressed: u64,
}

#[derive(Debug, Default)]
pub(crate) struct RefusalCoalescer {
    entries: HashMap<PathBuf, Entry>,
}

impl RefusalCoalescer {
    pub(crate) fn admit(
        &mut self,
        path: &Path,
        reason: &str,
        detail: &str,
        now: Instant,
    ) -> RefusalAdmission {
        if let Some(entry) = self.entries.get_mut(path) {
            if entry.reason == reason && entry.detail == detail {
                if now.saturating_duration_since(entry.emitted_at) < REFUSAL_COALESCE_WINDOW {
                    entry.suppressed = entry.suppressed.saturating_add(1);
                    return RefusalAdmission::Suppress;
                }
                let coalesced_repeats = std::mem::take(&mut entry.suppressed);
                entry.emitted_at = now;
                return RefusalAdmission::Emit {
                    coalesced_repeats,
                    flushed: None,
                };
            }
            let flushed = (entry.suppressed > 0).then(|| SuppressedRun {
                reason: entry.reason.clone(),
                detail: entry.detail.clone(),
                suppressed: entry.suppressed,
            });
            *entry = Entry {
                reason: reason.to_string(),
                detail: detail.to_string(),
                emitted_at: now,
                suppressed: 0,
            };
            return RefusalAdmission::Emit {
                coalesced_repeats: 0,
                flushed,
            };
        }
        if self.entries.len() >= MAX_TRACKED_PATHS {
            self.entries.clear();
        }
        self.entries.insert(
            path.to_path_buf(),
            Entry {
                reason: reason.to_string(),
                detail: detail.to_string(),
                emitted_at: now,
                suppressed: 0,
            },
        );
        RefusalAdmission::Emit {
            coalesced_repeats: 0,
            flushed: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REASON: &str = "doc_advanced_during_drain";
    const DETAIL: &str = "ops=1 requeued=true";

    #[test]
    fn first_refusal_is_emitted_and_identical_repeats_are_counted() {
        let mut coalescer = RefusalCoalescer::default();
        let path = Path::new("/doc-a.md");
        let t0 = Instant::now();
        assert_eq!(
            coalescer.admit(path, REASON, DETAIL, t0),
            RefusalAdmission::Emit {
                coalesced_repeats: 0,
                flushed: None
            }
        );
        // GH #242 rate: ~5.8 refusals/s. Inside the window none of them may write.
        for i in 1..=11 {
            assert_eq!(
                coalescer.admit(path, REASON, DETAIL, t0 + Duration::from_millis(170 * i)),
                RefusalAdmission::Suppress
            );
        }
        // Another document is tracked independently.
        assert!(matches!(
            coalescer.admit(Path::new("/doc-b.md"), REASON, DETAIL, t0),
            RefusalAdmission::Emit { .. }
        ));
    }

    #[test]
    fn a_repeat_after_the_window_carries_the_suppressed_count() {
        let mut coalescer = RefusalCoalescer::default();
        let path = Path::new("/doc-a.md");
        let t0 = Instant::now();
        coalescer.admit(path, REASON, DETAIL, t0);
        coalescer.admit(path, REASON, DETAIL, t0 + Duration::from_millis(500));
        coalescer.admit(path, REASON, DETAIL, t0 + Duration::from_millis(1000));
        assert_eq!(
            coalescer.admit(path, REASON, DETAIL, t0 + REFUSAL_COALESCE_WINDOW),
            RefusalAdmission::Emit {
                coalesced_repeats: 2,
                flushed: None
            }
        );
        // The window restarts at the emission.
        assert_eq!(
            coalescer.admit(
                path,
                REASON,
                DETAIL,
                t0 + REFUSAL_COALESCE_WINDOW + Duration::from_millis(10)
            ),
            RefusalAdmission::Suppress
        );
    }

    #[test]
    fn a_different_refusal_flushes_the_prior_run() {
        let mut coalescer = RefusalCoalescer::default();
        let path = Path::new("/doc-a.md");
        let t0 = Instant::now();
        coalescer.admit(path, REASON, DETAIL, t0);
        coalescer.admit(path, REASON, DETAIL, t0 + Duration::from_millis(100));
        assert_eq!(
            coalescer.admit(
                path,
                "shadow_replay_mismatch",
                "ops=2",
                t0 + Duration::from_millis(200)
            ),
            RefusalAdmission::Emit {
                coalesced_repeats: 0,
                flushed: Some(SuppressedRun {
                    reason: REASON.to_string(),
                    detail: DETAIL.to_string(),
                    suppressed: 1,
                }),
            }
        );
        // A changed detail is a different refusal, not a repeat.
        assert!(matches!(
            coalescer.admit(
                path,
                "shadow_replay_mismatch",
                "ops=3",
                t0 + Duration::from_millis(300)
            ),
            RefusalAdmission::Emit { flushed: None, .. }
        ));
    }

    #[test]
    fn the_tracked_path_table_is_bounded() {
        let mut coalescer = RefusalCoalescer::default();
        let t0 = Instant::now();
        for i in 0..(MAX_TRACKED_PATHS * 2) {
            coalescer.admit(&PathBuf::from(format!("/doc-{i}.md")), REASON, DETAIL, t0);
        }
        assert!(coalescer.entries.len() <= MAX_TRACKED_PATHS);
    }
}

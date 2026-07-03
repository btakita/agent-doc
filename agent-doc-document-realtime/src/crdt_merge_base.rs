//! Pure CRDT merge-base policy shared by realtime document persistence callers.
//!
//! This module owns the vocabulary and content-only decision tree for
//! classifying where a write cycle's CRDT merge base came from. Callers still
//! own sidecar IO, locks, logging sinks, pending-op discovery, and rebuild
//! effects.

use agent_doc_document::model_projection::{
    OverlayProjectionSource, first_diff_byte, overlay_carries_unbaselined_content,
    project_overlay_state_with_source,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrdtMergeBaseSource {
    Overlay,
    FallbackNoOverlay,
    FallbackOverlayDecodeError,
    FallbackOverlayProjectionMismatch,
    /// The overlay projection diverged from the cycle baseline and carried live
    /// editor content that must be preserved in the sidecar.
    OverlayAheadPreserved,
}

impl CrdtMergeBaseSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            CrdtMergeBaseSource::Overlay => "overlay",
            CrdtMergeBaseSource::FallbackNoOverlay => "fallback_no_overlay",
            CrdtMergeBaseSource::FallbackOverlayDecodeError => "fallback_overlay_decode_error",
            CrdtMergeBaseSource::FallbackOverlayProjectionMismatch => {
                "fallback_overlay_projection_mismatch"
            }
            CrdtMergeBaseSource::OverlayAheadPreserved => "overlay_ahead_preserved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrdtMergeBase {
    pub state: Vec<u8>,
    pub source: CrdtMergeBaseSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrdtMergeBaseResolution {
    pub base: CrdtMergeBase,
    pub markdown: String,
    pub events: Vec<CrdtMergeBaseEvent>,
    pub rebuild_overlay_to_baseline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrdtMergeBaseEvent {
    OverlayStale {
        overlay_len: usize,
        fallback_len: usize,
    },
    OverlayAheadPreserved {
        overlay_len: usize,
        fallback_len: usize,
        first_diff_offset: Option<usize>,
    },
    OverlayDiscarded {
        overlay_len: usize,
        baseline_len: usize,
        first_diff_offset: Option<usize>,
    },
    OverlayDecodeError {
        error: String,
        baseline_len: usize,
    },
    Final {
        source: CrdtMergeBaseSource,
        len: usize,
    },
}

impl CrdtMergeBaseEvent {
    pub fn log_message(&self, file: impl std::fmt::Display) -> String {
        match self {
            CrdtMergeBaseEvent::OverlayStale {
                overlay_len,
                fallback_len,
            } => format!(
                "crdt_merge_base_overlay_stale file={} overlay_len={} fallback_len={}",
                file, overlay_len, fallback_len
            ),
            CrdtMergeBaseEvent::OverlayAheadPreserved {
                overlay_len,
                fallback_len,
                first_diff_offset,
            } => format!(
                "crdt_merge_base_overlay_ahead_preserved file={} overlay_len={} fallback_len={} first_diff_offset={:?}",
                file, overlay_len, fallback_len, first_diff_offset
            ),
            CrdtMergeBaseEvent::OverlayDiscarded {
                overlay_len,
                baseline_len,
                first_diff_offset,
            } => format!(
                "overlay_discarded_bytes file={} overlay_len={} baseline_len={} first_diff_offset={:?}",
                file, overlay_len, baseline_len, first_diff_offset
            ),
            CrdtMergeBaseEvent::OverlayDecodeError {
                error,
                baseline_len,
            } => format!(
                "crdt_merge_base_overlay_decode_error file={} error={} overlay_discarded_bytes file={} baseline_len={}",
                file, error, file, baseline_len
            ),
            CrdtMergeBaseEvent::Final { source, len } => format!(
                "crdt_merge_base file={} source={} len={}",
                file,
                source.as_str(),
                len
            ),
        }
    }
}

/// Resolve the CRDT merge base from optional overlay-state bytes and the cycle
/// baseline.
///
/// The structured overlay sidecar is authoritative when it decodes and its
/// markdown projection matches the explicit cycle baseline. Divergence has two
/// opposite meanings:
///
/// - **overlay-behind / stale**: the overlay holds older content than the
///   baseline. Baseline wins, and the caller should rebuild the overlay sidecar
///   from that baseline.
/// - **overlay-ahead / concurrent-divergent / live**: the overlay carries
///   content the baseline lacks. The merge base still stays the committed
///   baseline, but the caller must preserve the live overlay sidecar.
///
/// `#crdtsvdom` (content form): the line-multiset scan only sees "ahead" when
/// the overlay holds a line the baseline lacks: additions and within-line edits.
/// It is blind to a pure deletion / within-line shrink (`overlay ⊆ baseline`),
/// which is content-identical to a genuinely older committed subset. True yrs
/// state-vector dominance cannot disambiguate this runtime sidecar because it is
/// a fresh lineage-free projection, so pending editor ops are the external
/// causal signal.
pub fn resolve_crdt_merge_base(
    overlay_bytes: Option<&[u8]>,
    fallback_markdown: &str,
    has_pending_editor_ops: bool,
) -> CrdtMergeBaseResolution {
    let mut events = Vec::new();
    let mut rebuild_overlay_to_baseline = false;

    let (markdown, source) = match overlay_bytes {
        None => (
            fallback_markdown.to_string(),
            CrdtMergeBaseSource::FallbackNoOverlay,
        ),
        Some(bytes) => match project_overlay_state_with_source(bytes, Some(fallback_markdown)) {
            Ok(projection)
                if projection.source == OverlayProjectionSource::Structured
                    && projection.markdown == fallback_markdown =>
            {
                (projection.markdown, CrdtMergeBaseSource::Overlay)
            }
            Ok(projection) if projection.source == OverlayProjectionSource::Structured => {
                let overlay_md = projection.markdown;
                events.push(CrdtMergeBaseEvent::OverlayStale {
                    overlay_len: overlay_md.len(),
                    fallback_len: fallback_markdown.len(),
                });
                if overlay_carries_unbaselined_content(
                    &overlay_md,
                    fallback_markdown,
                    has_pending_editor_ops,
                ) {
                    events.push(CrdtMergeBaseEvent::OverlayAheadPreserved {
                        overlay_len: overlay_md.len(),
                        fallback_len: fallback_markdown.len(),
                        first_diff_offset: first_diff_byte(&overlay_md, fallback_markdown),
                    });
                    (
                        fallback_markdown.to_string(),
                        CrdtMergeBaseSource::OverlayAheadPreserved,
                    )
                } else {
                    events.push(CrdtMergeBaseEvent::OverlayDiscarded {
                        overlay_len: overlay_md.len(),
                        baseline_len: fallback_markdown.len(),
                        first_diff_offset: first_diff_byte(&overlay_md, fallback_markdown),
                    });
                    rebuild_overlay_to_baseline = true;
                    (
                        fallback_markdown.to_string(),
                        CrdtMergeBaseSource::FallbackOverlayProjectionMismatch,
                    )
                }
            }
            Ok(projection) => {
                let OverlayProjectionSource::RebuiltFromFallback { error } = projection.source
                else {
                    unreachable!("structured overlay projections are handled above");
                };
                events.push(CrdtMergeBaseEvent::OverlayDecodeError {
                    error,
                    baseline_len: fallback_markdown.len(),
                });
                rebuild_overlay_to_baseline = true;
                (
                    projection.markdown,
                    CrdtMergeBaseSource::FallbackOverlayDecodeError,
                )
            }
            Err(err) => {
                events.push(CrdtMergeBaseEvent::OverlayDecodeError {
                    error: err.to_string(),
                    baseline_len: fallback_markdown.len(),
                });
                rebuild_overlay_to_baseline = true;
                (
                    fallback_markdown.to_string(),
                    CrdtMergeBaseSource::FallbackOverlayDecodeError,
                )
            }
        },
    };

    events.push(CrdtMergeBaseEvent::Final {
        source,
        len: markdown.len(),
    });

    CrdtMergeBaseResolution {
        base: CrdtMergeBase {
            state: agent_doc_merge::crdt::CrdtDoc::from_text(&markdown).encode_state(),
            source,
        },
        markdown,
        events,
        rebuild_overlay_to_baseline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_document::model_projection::overlay_state_from_markdown;

    fn decode_base_markdown(base: &CrdtMergeBase) -> String {
        agent_doc_merge::crdt::CrdtDoc::decode_state(&base.state)
            .unwrap()
            .to_text()
    }

    #[test]
    fn merge_base_sources_have_stable_log_labels() {
        let cases = [
            (CrdtMergeBaseSource::Overlay, "overlay"),
            (
                CrdtMergeBaseSource::FallbackNoOverlay,
                "fallback_no_overlay",
            ),
            (
                CrdtMergeBaseSource::FallbackOverlayDecodeError,
                "fallback_overlay_decode_error",
            ),
            (
                CrdtMergeBaseSource::FallbackOverlayProjectionMismatch,
                "fallback_overlay_projection_mismatch",
            ),
            (
                CrdtMergeBaseSource::OverlayAheadPreserved,
                "overlay_ahead_preserved",
            ),
        ];

        for (source, label) in cases {
            assert_eq!(source.as_str(), label);
        }
    }

    #[test]
    fn merge_base_uses_fallback_when_overlay_absent() {
        let fallback = "baseline\n";

        let resolved = resolve_crdt_merge_base(None, fallback, false);

        assert_eq!(resolved.markdown, fallback);
        assert_eq!(resolved.base.source, CrdtMergeBaseSource::FallbackNoOverlay);
        assert!(!resolved.rebuild_overlay_to_baseline);
        assert_eq!(decode_base_markdown(&resolved.base), fallback);
        assert_eq!(
            resolved.events,
            vec![CrdtMergeBaseEvent::Final {
                source: CrdtMergeBaseSource::FallbackNoOverlay,
                len: fallback.len()
            }]
        );
    }

    #[test]
    fn merge_base_uses_matching_overlay_projection() {
        let fallback = "baseline\n";
        let overlay = overlay_state_from_markdown(fallback);

        let resolved = resolve_crdt_merge_base(Some(&overlay), fallback, false);

        assert_eq!(resolved.markdown, fallback);
        assert_eq!(resolved.base.source, CrdtMergeBaseSource::Overlay);
        assert!(!resolved.rebuild_overlay_to_baseline);
        assert_eq!(decode_base_markdown(&resolved.base), fallback);
        assert_eq!(
            resolved.events,
            vec![CrdtMergeBaseEvent::Final {
                source: CrdtMergeBaseSource::Overlay,
                len: fallback.len()
            }]
        );
    }

    #[test]
    fn merge_base_discards_and_rebuilds_stale_overlay() {
        let overlay_markdown = "old line\n";
        let fallback = "old line\nnew line\n";
        let overlay = overlay_state_from_markdown(overlay_markdown);

        let resolved = resolve_crdt_merge_base(Some(&overlay), fallback, false);

        assert_eq!(resolved.markdown, fallback);
        assert_eq!(
            resolved.base.source,
            CrdtMergeBaseSource::FallbackOverlayProjectionMismatch
        );
        assert!(resolved.rebuild_overlay_to_baseline);
        assert_eq!(decode_base_markdown(&resolved.base), fallback);
        assert_eq!(
            resolved.events,
            vec![
                CrdtMergeBaseEvent::OverlayStale {
                    overlay_len: overlay_markdown.len(),
                    fallback_len: fallback.len()
                },
                CrdtMergeBaseEvent::OverlayDiscarded {
                    overlay_len: overlay_markdown.len(),
                    baseline_len: fallback.len(),
                    first_diff_offset: Some(9)
                },
                CrdtMergeBaseEvent::Final {
                    source: CrdtMergeBaseSource::FallbackOverlayProjectionMismatch,
                    len: fallback.len()
                },
            ]
        );
    }

    #[test]
    fn merge_base_preserves_overlay_ahead_with_pending_editor_ops() {
        let overlay_markdown = "remaining\n";
        let fallback = "remaining\ndeleted\n";
        let overlay = overlay_state_from_markdown(overlay_markdown);

        let resolved = resolve_crdt_merge_base(Some(&overlay), fallback, true);

        assert_eq!(resolved.markdown, fallback);
        assert_eq!(
            resolved.base.source,
            CrdtMergeBaseSource::OverlayAheadPreserved
        );
        assert!(!resolved.rebuild_overlay_to_baseline);
        assert_eq!(decode_base_markdown(&resolved.base), fallback);
        assert_eq!(
            resolved.events,
            vec![
                CrdtMergeBaseEvent::OverlayStale {
                    overlay_len: overlay_markdown.len(),
                    fallback_len: fallback.len()
                },
                CrdtMergeBaseEvent::OverlayAheadPreserved {
                    overlay_len: overlay_markdown.len(),
                    fallback_len: fallback.len(),
                    first_diff_offset: Some(10)
                },
                CrdtMergeBaseEvent::Final {
                    source: CrdtMergeBaseSource::OverlayAheadPreserved,
                    len: fallback.len()
                },
            ]
        );
    }

    #[test]
    fn merge_base_falls_back_and_rebuilds_on_foreign_overlay_state() {
        let fallback = "baseline\n";

        let resolved = resolve_crdt_merge_base(Some(b"not a crdt state"), fallback, false);

        assert_eq!(resolved.markdown, fallback);
        assert_eq!(
            resolved.base.source,
            CrdtMergeBaseSource::FallbackOverlayDecodeError
        );
        assert!(resolved.rebuild_overlay_to_baseline);
        assert_eq!(decode_base_markdown(&resolved.base), fallback);
        assert!(matches!(
            resolved.events.as_slice(),
            [
                CrdtMergeBaseEvent::OverlayDecodeError { baseline_len, .. },
                CrdtMergeBaseEvent::Final { source, len }
            ] if *baseline_len == fallback.len()
                && *source == CrdtMergeBaseSource::FallbackOverlayDecodeError
                && *len == fallback.len()
        ));
        assert!(
            resolved.events[0]
                .log_message("doc.md")
                .contains("crdt_merge_base_overlay_decode_error file=doc.md error=")
        );
    }
}

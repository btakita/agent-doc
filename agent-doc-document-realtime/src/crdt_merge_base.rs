//! Pure CRDT merge-base policy shared by realtime document persistence callers.
//!
//! This module owns the vocabulary for classifying where a write cycle's CRDT
//! merge base came from. Callers still own sidecar IO, locks, logging, and the
//! final encoded CRDT bytes.

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

#[derive(Debug, Clone)]
pub struct CrdtMergeBase {
    pub state: Vec<u8>,
    pub source: CrdtMergeBaseSource,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

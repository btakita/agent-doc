//! Editor-surface endpoint retirement: the controller's fence, the typed
//! rejection an editor acts on, and the muted-live-endpoint monitor
//! (`#tmuxautosyncretired`, `#surfaceretireformal`).
//!
//! Model: `formal/tla/PassiveTmuxSyncRetireNet.tla`. The controller's
//! `ControllerEditorSurfaceGraph` keeps, per `(project, client, surface)`
//! endpoint, the connection generation and sequence it last accepted and
//! whether the endpoint was retired by `editor_surface_forget`. A retired
//! endpoint is fenced at every generation `<=` its own, so retirement is a
//! correct fence for a frame the editor declared ended. A frame that is in fact
//! still live (a self-retirement bug, or a Remote Dev backend fallback resuming
//! on a frame a frontend handoff retired) must therefore come back at a NEWER
//! generation: the controller's typed [`SurfaceObservationRejection::RetiredEndpoint`]
//! receipt carries the generation to register above, and a live editor
//! re-registers at [`reregistration_generation`] and re-sends the same
//! observation, so it is muted for at most one round trip.

use serde::{Deserialize, Serialize};

/// Why the controller editor-surface graph fenced an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceObservationRejection {
    /// The exact `(client, surface)` endpoint was retired by
    /// `editor_surface_forget` at this or a newer connection generation. A
    /// still-live editor must re-register above the retired generation
    /// (`SurfaceObservationReceipt::reregister_above_generation`).
    RetiredEndpoint,
    /// The observation belongs to an older connection generation than the
    /// endpoint's current one (a reordered copy from before a re-registration).
    StaleGeneration,
    /// A duplicate or reordered observation within the current generation.
    StaleSequence,
}

impl SurfaceObservationRejection {
    /// The controller fence, in the order the model states it
    /// (`ControllerRecvObs` in `PassiveTmuxSyncRetireNet.tla`).
    pub fn classify(
        retired: bool,
        current_generation: u64,
        current_sequence: u64,
        generation: u64,
        sequence: u64,
    ) -> Option<Self> {
        if retired && generation <= current_generation {
            Some(Self::RetiredEndpoint)
        } else if generation < current_generation {
            Some(Self::StaleGeneration)
        } else if generation == current_generation && sequence <= current_sequence {
            Some(Self::StaleSequence)
        } else {
            None
        }
    }

    /// The `rejected=` value on `controller_editor_surface_observed`.
    pub fn label(self) -> &'static str {
        match self {
            Self::RetiredEndpoint => "retired_endpoint",
            Self::StaleGeneration => "stale_generation",
            Self::StaleSequence => "stale_sequence",
        }
    }
}

/// The generation a live editor re-registers at after a
/// [`SurfaceObservationRejection::RetiredEndpoint`] receipt naming
/// `retired_generation`: strictly above the retired one, and never below the
/// editor's own current generation (a concurrent rejection may have bumped it
/// already). Idempotent and monotone, so racing receipts converge.
pub fn reregistration_generation(current: u64, retired_generation: u64) -> u64 {
    current.max(retired_generation.saturating_add(1))
}

/// Consecutive `rejected=retired_endpoint` receipts after which the controller
/// reports a muted live endpoint.
///
/// One retired receipt is expected after any legitimate retirement the editor
/// did not see coming (the frame resumed, or a forget raced an observation),
/// and a re-registering editor may still have one more observation in flight
/// at the old generation. A third consecutive one means the editor ignored the
/// re-registration directive: a plugin that predates it, or a client that
/// keeps publishing while it believes the frame is not its own.
pub const RETIRED_ENDPOINT_MUTE_THRESHOLD: u32 = 3;

/// Runtime monitor of `LiveEndpointNeverMuted`: counts one endpoint's
/// consecutive retired-endpoint rejections.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetiredEndpointMonitor {
    consecutive: u32,
}

/// A live endpoint that kept publishing into its own retired endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutedLiveEndpoint {
    /// Consecutive retired-endpoint rejections (equal to the threshold).
    pub consecutive: u32,
}

impl RetiredEndpointMonitor {
    /// Fold one observation verdict for this endpoint. Returns the alert exactly
    /// once per streak, on the observation that reaches
    /// [`RETIRED_ENDPOINT_MUTE_THRESHOLD`]. Any other verdict ends the streak.
    pub fn record(
        &mut self,
        rejection: Option<SurfaceObservationRejection>,
    ) -> Option<MutedLiveEndpoint> {
        if rejection != Some(SurfaceObservationRejection::RetiredEndpoint) {
            self.consecutive = 0;
            return None;
        }
        self.consecutive = self.consecutive.saturating_add(1);
        (self.consecutive == RETIRED_ENDPOINT_MUTE_THRESHOLD).then_some(MutedLiveEndpoint {
            consecutive: self.consecutive,
        })
    }

    /// Consecutive retired-endpoint rejections in the current streak.
    pub fn consecutive(&self) -> u32 {
        self.consecutive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SurfaceObservationRejection as R;

    #[test]
    fn rejection_classifies_each_fence_in_model_order() {
        // (retired, current_generation, current_sequence, generation, sequence)
        assert_eq!(R::classify(true, 7, 3, 7, 4), Some(R::RetiredEndpoint));
        assert_eq!(R::classify(true, 7, 3, 6, 9), Some(R::RetiredEndpoint));
        assert_eq!(
            R::classify(true, 7, 3, 8, 1),
            None,
            "a newer generation re-registers"
        );
        assert_eq!(R::classify(false, 7, 3, 6, 9), Some(R::StaleGeneration));
        assert_eq!(R::classify(false, 7, 3, 7, 3), Some(R::StaleSequence));
        assert_eq!(R::classify(false, 7, 3, 7, 4), None);
    }

    #[test]
    fn rejection_serializes_as_the_log_label() {
        for rejection in [R::RetiredEndpoint, R::StaleGeneration, R::StaleSequence] {
            assert_eq!(
                serde_json::to_string(&rejection).unwrap(),
                format!("\"{}\"", rejection.label())
            );
        }
    }

    #[test]
    fn reregistration_lands_strictly_above_the_retired_generation() {
        assert_eq!(reregistration_generation(5, 5), 6);
        assert_eq!(
            reregistration_generation(9, 5),
            9,
            "already above: unchanged"
        );
        assert_eq!(
            reregistration_generation(6, 5),
            6,
            "racing receipts converge"
        );
        assert_eq!(reregistration_generation(1, u64::MAX), u64::MAX);
    }

    #[test]
    fn monitor_alerts_once_at_the_threshold_and_resets_on_any_other_verdict() {
        let mut monitor = RetiredEndpointMonitor::default();
        for _ in 1..RETIRED_ENDPOINT_MUTE_THRESHOLD {
            assert_eq!(monitor.record(Some(R::RetiredEndpoint)), None);
        }
        assert_eq!(
            monitor.record(Some(R::RetiredEndpoint)),
            Some(MutedLiveEndpoint {
                consecutive: RETIRED_ENDPOINT_MUTE_THRESHOLD
            })
        );
        assert_eq!(
            monitor.record(Some(R::RetiredEndpoint)),
            None,
            "one alert per streak, not one per rejection"
        );
        assert_eq!(monitor.record(None), None);
        assert_eq!(
            monitor.consecutive(),
            0,
            "an accepted observation ends the streak"
        );
        for _ in 1..RETIRED_ENDPOINT_MUTE_THRESHOLD {
            assert_eq!(monitor.record(Some(R::RetiredEndpoint)), None);
        }
        assert_eq!(monitor.record(Some(R::StaleSequence)), None);
        assert_eq!(
            monitor.consecutive(),
            0,
            "a duplicate is not a muted endpoint"
        );
        for _ in 1..RETIRED_ENDPOINT_MUTE_THRESHOLD {
            assert_eq!(monitor.record(Some(R::RetiredEndpoint)), None);
        }
        assert!(
            monitor.record(Some(R::RetiredEndpoint)).is_some(),
            "a new streak alerts again"
        );
    }
}

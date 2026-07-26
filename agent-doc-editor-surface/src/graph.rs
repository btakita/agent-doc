//! The reactive plane over [`SurfaceTracking`] (`#jbpluginlazilyeffects`).
//!
//! The editor reports what it sees; an `Effect` drives what tmux does about it.
//! Between them sits one state-machine cell holding the history fold and one
//! `Computed` over its state handle.
//!
//! What this replaces is not a slow path — it is a *coupling*. The plugin held
//! the previous observation in mutable fields, asked its own planner what to do,
//! and submitted a command. Every editor event had to remember all three steps,
//! each editor implemented them separately, and a submitted command re-observed
//! the world and returned one already-stale answer with nothing subscribed to it.
//! Here the plugin's whole job is [`EditorSurfaceState::observe`]. The intent is
//! derived, so it cannot be out of step with the observation that produced it,
//! and the consequence is an `Effect` gated on that derived value — it fires on
//! a transition and is idempotent otherwise, which removes the "someone forgot
//! to call it" failure mode rather than guarding against it.
//!
//! The scope is a [`ProcessScope`]: an editor surface belongs to a project root
//! for as long as the process serving it lives, and the observations arrive from
//! editor threads, so this is the thread-safe family.

use agent_doc_state_scope::ProcessScope;
use lazily::{Computed, Effect, ThreadSafeContext, ThreadSafeStateMachine};

use crate::{EditorSurface, SurfaceIntent, SurfaceTracking};

/// The folded state: what tmux was last reconciled against, plus the intent the
/// most recent observation implied.
///
/// Both halves live in one value because `advance` produces them together. A
/// version of this that kept the intent in its own cell would have to be written
/// in step with the tracking value by hand, which is the ordering hazard the
/// pure fold exists to make unwritable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFold {
    pub tracking: SurfaceTracking,
    pub intent: SurfaceIntent,
    /// Bumped on every non-idle intent, so two *identical* consecutive intents
    /// are still distinguishable.
    ///
    /// Without it, the equality guard on the state cell would swallow the second
    /// one: a layout that drifts, is reconciled, and drifts back to the same
    /// shape produces the same `Sync` value twice, and the second is a real
    /// consequence that must still fire. An idle observation does not bump it,
    /// so repeated no-ops stay silent.
    pub epoch: u64,
}

impl Default for SurfaceFold {
    fn default() -> Self {
        Self {
            tracking: SurfaceTracking::default(),
            intent: SurfaceIntent::Idle,
            epoch: 0,
        }
    }
}

/// The fold: one observation advances the whole state.
///
/// Total and pure — every observation produces a next state, and an observation
/// that implies nothing leaves the tracking value and the epoch alone while
/// clearing the intent back to [`SurfaceIntent::Idle`].
pub fn advance(current: &SurfaceFold, surface: &EditorSurface) -> Option<SurfaceFold> {
    let (tracking, intent) = current.tracking.advance(surface);
    let epoch = if intent.is_idle() {
        current.epoch
    } else {
        current.epoch.saturating_add(1)
    };
    Some(SurfaceFold {
        tracking,
        intent,
        epoch,
    })
}

/// One project root's editor-surface graph.
pub struct EditorSurfaceState {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<SurfaceFold, EditorSurface>,
    intent: Computed<SurfaceIntent>,
    _scope: Option<ProcessScope>,
}

impl Default for EditorSurfaceState {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorSurfaceState {
    /// Build the graph in its own process scope.
    pub fn new() -> Self {
        let scope = ProcessScope::new();
        let mut state = Self::new_in(&scope);
        state._scope = Some(scope);
        state
    }

    /// Build the graph inside a caller-owned process scope (`#stategraphjoin`).
    ///
    /// `ThreadSafeContext` is `Clone`, so a thread-safe scope is shared by
    /// handing out clones rather than by owning it — which is why this is the
    /// form other controller-lifetime state should use: the surface graph then
    /// sits in the same graph as the rest of the process's facts and can be
    /// derived across, instead of being an island with the right shape.
    pub fn new_in(scope: &ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        let machine = ThreadSafeStateMachine::new(&ctx, SurfaceFold::default(), advance);
        let state = machine.state_handle();
        let intent = ctx.computed(move |c| c.get(&state).intent.clone());
        Self {
            ctx,
            machine,
            intent,
            _scope: None,
        }
    }

    /// Record what the editor looks like now.
    ///
    /// The entire plugin-facing surface. Everything downstream updates because
    /// this observation arrived.
    pub fn observe(&self, surface: EditorSurface) {
        self.machine.send(&self.ctx, surface);
    }

    /// What tmux should do about the current surface.
    pub fn intent(&self) -> SurfaceIntent {
        self.ctx.get(&self.intent)
    }

    /// The whole folded state, for callers that want more than the intent.
    pub fn fold(&self) -> SurfaceFold {
        self.machine.state(&self.ctx)
    }

    /// Run `sink` whenever a non-idle intent is derived, and only then.
    ///
    /// This is the consequence seam: the caller supplies the tmux action, the
    /// graph decides when it happens.
    ///
    /// The effect body is deliberately the *whole* side effect. An `Effect`
    /// whose body only assigns a value should have been a `Computed`; this one
    /// talks to tmux, which is what an effect is for.
    ///
    /// Returns the backing [`Effect`] handle. It is `Copy` and its lifetime is
    /// the scope's, **not** the handle's — dropping it does not unsubscribe, so
    /// a caller that wants to stop driving tmux before the scope ends must pass
    /// it to [`Self::stop`].
    pub fn on_intent(&self, sink: impl Fn(&SurfaceIntent) + Send + Sync + 'static) -> Effect {
        let state = self.machine.state_handle();
        self.ctx.effect(move |c| {
            let fold = c.get(&state);
            if fold.intent.is_idle() {
                return;
            }
            sink(&fold.intent);
        })
    }

    /// Unsubscribe a sink returned by [`Self::on_intent`].
    pub fn stop(&self, effect: &Effect) {
        self.ctx.dispose_effect(effect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SurfaceColumn;
    use std::sync::{Arc, Mutex};

    fn surface(focused: &str, columns: &[&[&str]]) -> EditorSurface {
        let columns: Vec<SurfaceColumn> = columns
            .iter()
            .map(|files| SurfaceColumn::new(files.iter().copied()))
            .collect();
        let visible = columns
            .iter()
            .flat_map(|column| column.files.iter().cloned())
            .collect();
        EditorSurface {
            focused: focused.to_string(),
            visible,
            columns,
            layout_synced: Some(true),
            force_reconcile: false,
        }
    }

    fn recorder() -> (Arc<Mutex<Vec<SurfaceIntent>>>, impl Fn(&SurfaceIntent)) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let seen = Arc::clone(&seen);
            move |intent: &SurfaceIntent| seen.lock().unwrap().push(intent.clone())
        };
        (seen, sink)
    }

    #[test]
    fn the_intent_is_derived_from_the_observation() {
        let state = EditorSurfaceState::new();
        assert_eq!(state.intent(), SurfaceIntent::Idle);

        state.observe(surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        assert!(matches!(state.intent(), SurfaceIntent::Sync { .. }));

        state.observe(surface("/b.md", &[&["/a.md"], &["/b.md"]]));
        assert_eq!(
            state.intent(),
            SurfaceIntent::Focus {
                document: "/b.md".to_string()
            }
        );
    }

    #[test]
    fn the_effect_fires_on_each_real_consequence_and_stays_silent_otherwise() {
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);
        assert!(
            seen.lock().unwrap().is_empty(),
            "an idle graph must not drive tmux on subscribe"
        );

        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        state.observe(visible.clone());
        state.observe(visible.clone());
        state.observe(visible);
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "repeat observations of the same surface are one consequence, not three"
        );

        state.observe(surface("/b.md", &[&["/a.md"], &["/b.md"]]));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[1],
            SurfaceIntent::Focus {
                document: "/b.md".to_string()
            }
        );
    }

    #[test]
    fn a_returning_layout_fires_again() {
        // A layout drifts to one column and back to two. The two `Sync` intents
        // are equal values, but the fold in between differs, so the ordinary
        // equality guard already lets the third through. Pinned because it is
        // the shape a reader expects to be at risk — the guard compares against
        // the immediately previous value, not against any historical one.
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);

        let two_columns = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let one_column = surface("/a.md", &[&["/a.md"]]);
        state.observe(two_columns.clone());
        state.observe(one_column);
        state.observe(two_columns);

        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn a_repeated_forced_reconcile_fires_every_time() {
        // THIS is what the epoch is for, and the only shape that needs it: two
        // *consecutive* observations that fold to an identical value with a
        // non-idle intent. The operator pressing Sync Tmux Layout twice produces
        // the same tracking value and the same `Focus` intent both times, so
        // without the epoch the state cell's equality guard swallows the second
        // press and nothing happens.
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);

        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        state.observe(visible.clone());
        let forced = EditorSurface {
            force_reconcile: true,
            ..visible
        };
        state.observe(forced.clone());
        state.observe(forced.clone());
        state.observe(forced);

        assert_eq!(
            seen.lock().unwrap().len(),
            4,
            "every explicit reconcile must reach tmux, including consecutive identical ones"
        );
    }

    #[test]
    fn repeated_proven_drift_reconciles_every_time() {
        // The same shape arriving from tmux rather than the operator: the
        // controller keeps reporting that the panes do not match a layout the
        // editor never changed. Each report is a fresh consequence.
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);

        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        state.observe(visible.clone());
        let drifted = EditorSurface {
            layout_synced: Some(false),
            ..visible
        };
        state.observe(drifted.clone());
        state.observe(drifted);

        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn an_inert_observation_drives_nothing() {
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);
        state.observe(EditorSurface::default());
        assert!(seen.lock().unwrap().is_empty());
        assert_eq!(state.fold(), SurfaceFold::default());
    }

    #[test]
    fn a_stopped_sink_no_longer_drives_tmux() {
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let effect = state.on_intent(sink);
        state.observe(surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        assert_eq!(seen.lock().unwrap().len(), 1);

        // `Effect` is a `Copy` handle into the scope's graph, not an RAII guard:
        // dropping it leaves the subscription live. Unsubscribing is explicit.
        drop(effect);
        state.observe(surface("/b.md", &[&["/a.md"], &["/b.md"]]));
        assert_eq!(
            seen.lock().unwrap().len(),
            2,
            "dropping the handle must not silently unsubscribe"
        );

        state.stop(&effect);
        state.observe(surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        assert_eq!(
            seen.lock().unwrap().len(),
            2,
            "a stopped consequence must not keep firing"
        );
    }
}

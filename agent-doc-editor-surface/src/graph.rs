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
use lazily::{
    Computed, DependencyAvailability, Effect, ThreadSafeContext, ThreadSafeDependencyMap,
    ThreadSafeStateMachine,
};

use crate::{
    CurrentDocumentAuthority, DocumentAuthority, EditorSurface, SurfaceIntent, SurfaceTracking,
    TmuxLayout, layout_matches,
};

/// One observation of the whole mirror: what the editor shows, and whether tmux
/// currently matches it.
///
/// The machine's event. `layout_matches` is *derived* before the event is sent
/// rather than reported by the caller, so neither side of the mirror has to know
/// the other's state to produce a correct decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceObservation {
    pub surface: EditorSurface,
    pub layout_matches: Option<bool>,
}

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
pub fn advance(current: &SurfaceFold, observation: &SurfaceObservation) -> Option<SurfaceFold> {
    let (tracking, intent) = current
        .tracking
        .advance(&observation.surface, observation.layout_matches);
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
    /// What the editor reports. An observation, written by the plugin.
    editor: lazily::Source<EditorSurface>,
    /// What tmux is showing. An observation, written by the controller.
    tmux: lazily::Source<Option<TmuxLayout>>,
    /// Derived across both sides of the mirror: has tmux drifted from the layout
    /// the editor is showing? This is the value that used to be a field on
    /// `EditorSurface`, asked of an editor that cannot know it.
    layout_matches: Computed<Option<bool>>,
    /// Controller-owned input, independently materialized for every open
    /// document by the native adapter.
    document_authorities: ThreadSafeDependencyMap<String, DocumentAuthority>,
    /// Selected document × its controller-owned authority.
    current_document_authority: Computed<CurrentDocumentAuthority>,
    machine: ThreadSafeStateMachine<SurfaceFold, SurfaceObservation>,
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
        let editor = ctx.source(EditorSurface::default());
        let tmux = ctx.source(None::<TmuxLayout>);
        let document_authorities = ThreadSafeDependencyMap::new(&ctx);
        let layout_matches = ctx.computed(move |c| {
            let surface = c.get(&editor);
            let tmux = c.get(&tmux);
            layout_matches(&surface, tmux.as_ref())
        });
        let current_document_authority = {
            let authorities = document_authorities.clone();
            ctx.computed(move |c| {
                let document = c.get(&editor).focused.trim().to_string();
                if document.is_empty() {
                    return CurrentDocumentAuthority::default();
                }
                CurrentDocumentAuthority {
                    authority: match authorities.observe_dependency(c, document.clone()) {
                        DependencyAvailability::Unavailable => None,
                        DependencyAvailability::Available(authority) => Some(authority),
                    },
                    document: Some(document),
                }
            })
        };
        let machine = ThreadSafeStateMachine::new(&ctx, SurfaceFold::default(), advance);
        let state = machine.state_handle();
        let intent = ctx.computed(move |c| c.get(&state).intent.clone());
        Self {
            ctx,
            editor,
            tmux,
            layout_matches,
            document_authorities,
            current_document_authority,
            machine,
            intent,
            _scope: None,
        }
    }

    /// Fold the current state of both observations into the machine.
    ///
    /// Reads the derived mirror comparison rather than taking it as an argument,
    /// so an editor event and a tmux event produce the same decision from the
    /// same two cells.
    fn fold_current(&self) {
        let observation = SurfaceObservation {
            surface: self.ctx.get(&self.editor),
            layout_matches: self.ctx.get(&self.layout_matches),
        };
        self.machine.send(&self.ctx, observation);
    }

    /// Record what the editor looks like now.
    ///
    /// The entire plugin-facing surface. Everything downstream updates because
    /// this observation arrived.
    pub fn observe(&self, surface: EditorSurface) {
        self.ctx.set(&self.editor, surface);
        self.fold_current();
    }

    /// Record both halves of the mirror at once, and fold once.
    ///
    /// For the caller that pulls the controller's observation alongside the
    /// editor's rather than waiting to be pushed it. Writing the two sources
    /// separately would fold twice, and the first fold would compare the new
    /// tmux layout against the *previous* editor surface — a comparison neither
    /// side ever made, which can derive a `Sync` that nothing observed.
    pub fn observe_with_tmux(&self, surface: EditorSurface, tmux: Option<TmuxLayout>) {
        self.ctx.set(&self.tmux, tmux);
        self.ctx.set(&self.editor, surface);
        self.fold_current();
    }

    /// Record what tmux is showing now.
    ///
    /// The controller's half of the mirror, and the capability the previous
    /// shape did not have: tmux drifting is an event in its own right. The
    /// consequence follows from the controller's observation alone, with no
    /// editor event required and nothing for the plugin to report.
    pub fn observe_tmux(&self, layout: Option<TmuxLayout>) {
        self.ctx.set(&self.tmux, layout);
        self.fold_current();
    }

    /// Record the native/controller authority for one open document.
    pub fn observe_document_authority(&self, authority: DocumentAuthority) {
        let document = authority.document.clone();
        if self
            .document_authority(&document)
            .is_some_and(|current| current.revision > authority.revision)
        {
            return;
        }
        self.document_authorities
            .publish(&self.ctx, document, authority);
    }

    /// Controller authority for a specific document, if its native worker has
    /// published at least one value.
    pub fn document_authority(&self, document: &str) -> Option<DocumentAuthority> {
        self.document_authorities
            .observe(&self.ctx, &document.to_string())
            .and_then(|availability| match availability {
                DependencyAvailability::Unavailable => None,
                DependencyAvailability::Available(authority) => Some(authority),
            })
    }

    /// The selected editor document joined with its independently supplied
    /// controller authority.
    pub fn current_document_authority(&self) -> CurrentDocumentAuthority {
        self.ctx.get(&self.current_document_authority)
    }

    /// Whether tmux matches the layout the editor is showing, as currently
    /// derived. `None` until the controller has reported a tmux layout.
    pub fn layout_matches(&self) -> Option<bool> {
        self.ctx.get(&self.layout_matches)
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
    use crate::{DocumentAuthorityReadiness, SurfaceColumn};
    use agent_doc_turn::{CyclePhase, cp_projection::TurnProjection};
    use std::sync::{Arc, Mutex};

    fn surface(focused: &str, columns: &[&[&str]]) -> EditorSurface {
        let columns: Vec<SurfaceColumn> = columns
            .iter()
            .map(|files| SurfaceColumn::new(files.iter().copied()))
            .collect();
        let visible = columns
            .iter()
            .flat_map(|column| column.files.iter().cloned())
            .collect::<Vec<_>>();
        EditorSurface {
            focused: focused.to_string(),
            open: visible.clone(),
            visible,
            columns,
            force_reconcile: false,
            focus_only: false,
        }
    }

    /// A tmux layout that mirrors the editor's, so the derived comparison says
    /// "matches" unless a test writes drift.
    fn mirrored(surface: &EditorSurface) -> TmuxLayout {
        TmuxLayout {
            columns: surface.columns.clone(),
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
    fn focused_document_before_first_authority_publish_reacts_without_membership_epoch() {
        let state = EditorSurfaceState::new();
        state.observe(surface("/a.md", &[&["/a.md"], &["/b.md"]]));

        let pending_a = DocumentAuthority::pending("/a.md");
        state.observe_document_authority(pending_a.clone());
        assert_eq!(
            state.current_document_authority(),
            CurrentDocumentAuthority {
                document: Some("/a.md".to_string()),
                authority: Some(pending_a),
            }
        );

        let ready_b = DocumentAuthority {
            document: "/b.md".to_string(),
            readiness: DocumentAuthorityReadiness::Ready,
            turn: Some(TurnProjection::from_phase(CyclePhase::WriteApplied)),
            error: None,
            revision: 2,
        };
        state.observe_document_authority(ready_b.clone());
        state.observe(surface("/b.md", &[&["/a.md"], &["/b.md"]]));
        assert_eq!(
            state.current_document_authority(),
            CurrentDocumentAuthority {
                document: Some("/b.md".to_string()),
                authority: Some(ready_b),
            }
        );
    }

    #[test]
    fn stale_authority_cannot_replace_a_newer_document_projection() {
        let state = EditorSurfaceState::new();
        state.observe(surface("/a.md", &[&["/a.md"]]));
        let ready = DocumentAuthority {
            document: "/a.md".to_string(),
            readiness: DocumentAuthorityReadiness::Ready,
            turn: Some(TurnProjection::from_phase(CyclePhase::ResponseCaptured)),
            error: None,
            revision: 5,
        };
        state.observe_document_authority(ready.clone());
        state.observe_document_authority(DocumentAuthority {
            document: "/a.md".to_string(),
            readiness: DocumentAuthorityReadiness::Unavailable,
            turn: None,
            error: Some("stale worker".to_string()),
            revision: 4,
        });

        assert_eq!(state.document_authority("/a.md"), Some(ready));
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
        // controller keeps reporting panes that do not match a layout the editor
        // never changed. Each report is a fresh consequence.
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);

        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        state.observe_tmux(Some(mirrored(&visible)));
        state.observe(visible);

        let drifted = TmuxLayout {
            columns: vec![SurfaceColumn::new(["/b.md"]), SurfaceColumn::new(["/a.md"])],
        };
        state.observe_tmux(Some(drifted.clone()));
        state.observe_tmux(Some(drifted));

        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn tmux_drift_alone_drives_a_reconcile_with_no_editor_event() {
        // The capability the previous shape did not have. The editor reports
        // nothing new; the controller observes that the panes no longer mirror
        // the layout, and the consequence follows from that observation alone.
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);

        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        state.observe_tmux(Some(mirrored(&visible)));
        state.observe(visible.clone());
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "the first sighting reconciles"
        );
        assert_eq!(state.layout_matches(), Some(true));

        state.observe_tmux(Some(TmuxLayout {
            columns: vec![SurfaceColumn::new(["/b.md"]), SurfaceColumn::new(["/a.md"])],
        }));

        assert_eq!(state.layout_matches(), Some(false));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "drift is an event in its own right");
        assert!(matches!(seen[1], SurfaceIntent::Sync { .. }));
    }

    #[test]
    fn an_unreported_tmux_layout_never_reads_as_drift() {
        let state = EditorSurfaceState::new();
        let (seen, sink) = recorder();
        let _effect = state.on_intent(sink);

        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        state.observe(visible.clone());
        assert_eq!(state.layout_matches(), None);
        state.observe(visible);

        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "with no tmux observation the repeat is still idle, not a reconcile"
        );
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
        let _ = effect;
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

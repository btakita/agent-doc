//! Project-scoped editor-surface graphs and the tmux consequence they drive
//! (`#jbpluginlazilyeffects`).
//!
//! [`agent_doc_editor_surface`] is pure: it folds observations and derives an
//! intent. This crate is the half that touches the world — it owns one graph per
//! project root and subscribes the `Effect` that turns a derived
//! [`SurfaceIntent`] into a Project Controller command.
//!
//! The registry is a plain map of graph handles rather than a keyed reactive
//! collection, and that is deliberate rather than a shortcut. A keyed reactive
//! map fits when every entry's value is a pure function of that entry's
//! observations, as with the controller's retained-write verdicts. This fold is
//! history-dependent — the intent depends on what tmux was last reconciled
//! against — so each root needs its own state machine, and a `Computed` cannot
//! read its own previous value. What matters for `#stategraphjoin` is that every
//! entry is built with [`EditorSurfaceState::new_in`] against the registry's
//! **one** process scope, so the cells share a graph; only the membership
//! bookkeeping is ordinary data.
//!
//! The consequence is a constructor parameter, not a hardcoded call. Driving
//! tmux is the one thing these tests must not do — a test that reconciles a
//! layout would rearrange the panes of whoever is running it — so [`Registry`]
//! takes its runner and the tests supply a recording one. The graph, the
//! per-root history, and the membership rules are then exercised for real, with
//! only the tmux command replaced.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

use agent_doc_editor_surface::{
    EditorSurface, EditorSurfaceState, SurfaceColumn, SurfaceIntent, TmuxLayout,
};
use agent_doc_state_scope::ProcessScope;
use anyhow::{Context as _, Result};
use serde::Serialize;

/// What one observation did: the intent it implied, and what the tmux
/// consequence reported back.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceObservationReceipt {
    pub intent: SurfaceIntent,
    /// `true` when the observation implied no tmux consequence at all — the
    /// surface was inert, or identical to the one tmux was last reconciled
    /// against.
    pub idle: bool,
    /// The consequence's reply, when one ran. `None` for an idle observation,
    /// which is the point: an unchanged surface costs nothing.
    pub outcome: Option<String>,
    /// Set when the consequence ran and failed. The observation is still
    /// recorded — the editor reported the truth, and a failed tmux command does
    /// not make it untrue.
    pub error: Option<String>,
}

/// The tmux consequence of a derived intent.
pub type IntentRunner = Arc<dyn Fn(&Path, &SurfaceIntent) -> Result<String> + Send + Sync>;

/// The controller's half of the mirror, pulled for one editor observation.
///
/// `observe_tmux` is the push form: the controller notices drift and writes it.
/// A plugin process holds its own registry, and no controller writes into it, so
/// its tmux side would stay unobserved forever and proven drift would never
/// reconcile — the plugin would emit `Focus` for a layout tmux no longer shows.
/// This is the pull form of the same fact. It is still the *controller's*
/// observation, which is the property that matters: the editor never reports
/// whether tmux agrees with it.
///
/// `None` means "not asked, or no answer" — deliberately distinct from "tmux
/// matches" and from "tmux drifted" (`#idlerevisionreactive`).
pub type TmuxLayoutProbe = Arc<dyn Fn(&Path, &EditorSurface) -> Option<TmuxLayout> + Send + Sync>;

type SurfaceObserver = Arc<dyn Fn(&Path, EditorSurface) + Send + Sync>;

#[derive(Default)]
struct DeferredRoot {
    latest: Option<EditorSurface>,
    running: bool,
}

/// Per-root latest-wins dispatcher for editor observations.
///
/// Controller probes and tmux consequences can block on a route-owned actor.
/// They must therefore never run inside an editor's native-call lease. One
/// worker per active root preserves that root's observation order while a
/// newer queued surface replaces any superseded surface that has not started.
struct DeferredSurfaceDispatcher {
    roots: Mutex<HashMap<PathBuf, DeferredRoot>>,
    observe: SurfaceObserver,
}

impl DeferredSurfaceDispatcher {
    fn new(observe: SurfaceObserver) -> Arc<Self> {
        Arc::new(Self {
            roots: Mutex::new(HashMap::new()),
            observe,
        })
    }

    fn roots(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, DeferredRoot>> {
        self.roots.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn enqueue(self: &Arc<Self>, root: PathBuf, surface: EditorSurface) -> Result<()> {
        let should_start = {
            let mut roots = self.roots();
            let entry = roots.entry(root.clone()).or_default();
            entry.latest = Some(surface);
            if entry.running {
                false
            } else {
                entry.running = true;
                true
            }
        };
        if !should_start {
            return Ok(());
        }

        let dispatcher = Arc::clone(self);
        let worker_root = root.clone();
        if let Err(error) = thread::Builder::new()
            .name("agent-doc-editor-surface".to_string())
            .spawn(move || dispatcher.run_root(worker_root))
        {
            if let Some(entry) = self.roots().get_mut(&root) {
                entry.running = false;
            }
            anyhow::bail!(
                "spawn editor-surface worker for {}: {error}",
                root.display()
            );
        }
        Ok(())
    }

    fn run_root(&self, root: PathBuf) {
        loop {
            let Some(surface) = ({
                let mut roots = self.roots();
                let Some(entry) = roots.get_mut(&root) else {
                    return;
                };
                match entry.latest.take() {
                    Some(surface) => Some(surface),
                    None => {
                        roots.remove(&root);
                        None
                    }
                }
            }) else {
                return;
            };
            (self.observe)(&root, surface);
        }
    }
}

struct RootSurface {
    state: EditorSurfaceState,
    /// The subscription that drives the consequence. `Effect` is a `Copy` handle
    /// rather than an RAII guard, so this records ownership; dropping the entry
    /// goes through [`Registry::forget`] to unsubscribe.
    consequence: lazily::Effect,
    /// What the effect's last run reported. The effect fires synchronously
    /// inside `observe`, so this is written and read within one call rather
    /// than polled.
    outcome: Arc<Mutex<Option<Result<String, String>>>>,
}

/// One process's editor-surface graphs, keyed by project root.
pub struct Registry {
    scope: ProcessScope,
    run_intent: IntentRunner,
    probe_tmux: TmuxLayoutProbe,
    roots: Mutex<HashMap<PathBuf, RootSurface>>,
}

impl Registry {
    /// A registry whose tmux side is only ever written by [`Self::observe_tmux`].
    pub fn new(run_intent: IntentRunner) -> Self {
        Self::with_tmux_probe(run_intent, Arc::new(|_, _| None))
    }

    /// A registry that also pulls the controller's tmux observation for each
    /// editor observation, so drift reconciles in a process no controller
    /// pushes into.
    pub fn with_tmux_probe(run_intent: IntentRunner, probe_tmux: TmuxLayoutProbe) -> Self {
        Self {
            scope: ProcessScope::new(),
            run_intent,
            probe_tmux,
            roots: Mutex::new(HashMap::new()),
        }
    }

    fn roots(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, RootSurface>> {
        // A poisoned registry means another thread panicked mid-observation. The
        // graph itself is still consistent, so recover rather than propagate a
        // panic into an editor's UI thread.
        self.roots.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record what the editor at `project_root` looks like now, and return what
    /// that implied.
    ///
    /// The plugin's entire job. Whether tmux does anything, and what, is derived
    /// here — the caller does not decide, debounce, or dedup.
    pub fn observe(
        &self,
        project_root: &Path,
        surface: EditorSurface,
    ) -> SurfaceObservationReceipt {
        let should_probe = {
            let roots = self.roots();
            roots
                .get(project_root)
                .is_some_and(|entry| entry.state.fold().tracking.requires_tmux_probe(&surface))
        };
        // A first or changed layout already derives Sync, so publish it without
        // paying for a controller read first. Repeated layouts retain the probe
        // that detects tmux-only drift. Never hold the registry lock across that
        // round trip: one slow root must not serialize the others.
        let tmux = should_probe
            .then(|| (self.probe_tmux)(project_root, &surface))
            .flatten();
        self.with_entry(project_root, move |entry| {
            entry.state.observe_with_tmux(surface, tmux)
        })
    }

    /// Record what tmux is showing at `project_root`.
    ///
    /// The controller's half of the mirror. Drift the controller observes drives
    /// a reconcile on its own, with nothing for the editor to report — which is
    /// what the plugin-reported `layout_synced` field was standing in for, badly.
    pub fn observe_tmux(
        &self,
        project_root: &Path,
        layout: Option<TmuxLayout>,
    ) -> SurfaceObservationReceipt {
        self.with_entry(project_root, |entry| entry.state.observe_tmux(layout))
    }

    fn with_entry(
        &self,
        project_root: &Path,
        write: impl FnOnce(&RootSurface),
    ) -> SurfaceObservationReceipt {
        let root = project_root.to_path_buf();
        let mut roots = self.roots();
        let entry = roots.entry(root.clone()).or_insert_with(|| {
            let state = EditorSurfaceState::new_in(&self.scope);
            let outcome: Arc<Mutex<Option<Result<String, String>>>> = Arc::new(Mutex::new(None));
            let consequence = {
                let outcome = Arc::clone(&outcome);
                let run_intent = Arc::clone(&self.run_intent);
                let root = root.clone();
                state.on_intent(move |intent| {
                    let result = run_intent(&root, intent).map_err(|err| format!("{err:#}"));
                    if let Ok(mut slot) = outcome.lock() {
                        *slot = Some(result);
                    }
                })
            };
            RootSurface {
                state,
                consequence,
                outcome,
            }
        });

        if let Ok(mut slot) = entry.outcome.lock() {
            *slot = None;
        }
        write(entry);
        let intent = entry.state.intent();
        let outcome = entry.outcome.lock().ok().and_then(|slot| slot.clone());

        let (outcome, error) = match outcome {
            Some(Ok(output)) => (Some(output), None),
            Some(Err(message)) => (None, Some(message)),
            None => (None, None),
        };
        SurfaceObservationReceipt {
            idle: intent.is_idle(),
            intent,
            outcome,
            error,
        }
    }

    /// Record an observation supplied as JSON.
    ///
    /// The shape both editor plugins call. Keeping the boundary at JSON means
    /// the JetBrains and VS Code sides send the same document, so the rule
    /// cannot drift between them the way two hand-written planners did.
    pub fn observe_from_json(
        &self,
        project_root: &Path,
        surface_json: &str,
    ) -> Result<SurfaceObservationReceipt> {
        let surface: EditorSurface =
            serde_json::from_str(surface_json).context("parse editor surface json")?;
        Ok(self.observe(project_root, surface))
    }

    /// [`Self::observe_from_json`] with the receipt serialized back to JSON.
    pub fn observe_json(&self, project_root: &Path, surface_json: &str) -> Result<String> {
        let receipt = self.observe_from_json(project_root, surface_json)?;
        serde_json::to_string(&receipt).context("serialize editor surface receipt")
    }

    /// Forget a project root's graph — the editor closed the project.
    ///
    /// Unsubscribes its consequence and discards its history, so a reopened
    /// project starts from no reconciled layout.
    pub fn forget(&self, project_root: &Path) -> bool {
        let Some(entry) = self.roots().remove(project_root) else {
            return false;
        };
        entry.state.stop(&entry.consequence);
        true
    }
}

/// Drive the tmux consequence of `intent` through the Project Controller.
fn run_intent_via_controller(root: &Path, intent: &SurfaceIntent) -> Result<String> {
    match intent {
        SurfaceIntent::Idle => Ok(String::new()),
        SurfaceIntent::Focus { document } => {
            let receipt = agent_doc_controller_io::project_controller::focus_document_pane(
                root,
                Path::new(document),
            )?;
            serde_json::to_string(&receipt).context("serialize focus receipt")
        }
        SurfaceIntent::Sync { columns, document } => {
            let receipt = agent_doc_controller_io::project_controller::sync_tmux_layout(
                root,
                agent_doc_controller_io::project_controller::ControllerTmuxLayoutSyncInvocation {
                    // The controller's column wire format is one comma-joined
                    // string per column, the same shape as `agent-doc sync --col`.
                    columns: columns
                        .iter()
                        .map(|column| column.files.join(","))
                        .collect(),
                    window: None,
                    focus: Some(document.clone()),
                    // Passive: an editor reporting what it looks like must never
                    // cold-start a session, and must not collapse a layout that
                    // still has protected panes in it.
                    no_autostart: true,
                    exact_visible: true,
                    caller_kind: "automatic".to_string(),
                },
            )?;
            serde_json::to_string(&receipt).context("serialize sync receipt")
        }
    }
}

/// Ask the controller what tmux is showing, expressed as the mirror's other side.
///
/// The controller answers "does tmux show this layout, and if not, which
/// documents are in its panes" — so a mismatch becomes the tmux layout it
/// actually observed, and a match becomes the surface itself, which is what
/// "tmux shows this" means. Both are the controller's observation either way;
/// neither is the editor reporting on tmux.
///
/// A surface with fewer than two columns is not probed. Column *arrangement* is
/// what can drift, a one-column surface has none, and the probe is a round trip
/// on the editor's event path — so the answer would cost more than it is worth.
/// That returns `None` (unknown), not `Some(matching)`: claiming a match nobody
/// checked is the inversion `#idlerevisionreactive` warns about.
fn probe_tmux_via_controller(root: &Path, surface: &EditorSurface) -> Option<TmuxLayout> {
    if surface.columns.len() < 2 {
        return None;
    }
    let invocation =
        agent_doc_controller_io::project_controller::ControllerTmuxLayoutSyncStateInvocation {
            // The controller's column wire format is one comma-joined string per
            // column, the same shape as `agent-doc sync --col`.
            columns: surface
                .columns
                .iter()
                .map(|column| column.files.join(","))
                .collect(),
            window: None,
            focus: Some(surface.focused.clone()),
        };
    let report =
        match agent_doc_controller_io::project_controller::tmux_layout_sync_state(root, invocation)
        {
            Ok(report) => report,
            Err(err) => {
                // Not knowing is a distinct answer from "drifted". Treating an
                // unreachable controller as drift would reconcile the layout on
                // every editor event while the controller is down.
                eprintln!("[editor-surface] tmux layout probe unavailable: {err:#}");
                return None;
            }
        };
    if report.synced {
        return Some(TmuxLayout {
            columns: surface.columns.clone(),
        });
    }
    Some(TmuxLayout {
        columns: report
            .actual_documents
            .into_iter()
            .map(|document| SurfaceColumn {
                files: vec![document],
            })
            .collect(),
    })
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    Registry::with_tmux_probe(
        Arc::new(run_intent_via_controller),
        Arc::new(probe_tmux_via_controller),
    )
});

static DEFERRED_SURFACES: LazyLock<Arc<DeferredSurfaceDispatcher>> = LazyLock::new(|| {
    DeferredSurfaceDispatcher::new(Arc::new(|root, surface| {
        let _ = REGISTRY.observe(root, surface);
    }))
});

/// Record an editor-surface observation for the process-wide registry.
pub fn observe(project_root: &Path, surface: EditorSurface) -> SurfaceObservationReceipt {
    REGISTRY.observe(project_root, surface)
}

/// Validate and enqueue an editor-surface observation without waiting for its
/// controller probe or tmux consequence.
pub fn enqueue_from_json(project_root: &Path, surface_json: &str) -> Result<()> {
    let surface: EditorSurface =
        serde_json::from_str(surface_json).context("parse editor surface json")?;
    DEFERRED_SURFACES.enqueue(project_root.to_path_buf(), surface)
}

/// Record a tmux-layout observation for the process-wide registry.
pub fn observe_tmux(project_root: &Path, layout: Option<TmuxLayout>) -> SurfaceObservationReceipt {
    REGISTRY.observe_tmux(project_root, layout)
}

/// JSON-input form of [`observe`] — the shape both editor plugins call.
pub fn observe_from_json(
    project_root: &Path,
    surface_json: &str,
) -> Result<SurfaceObservationReceipt> {
    REGISTRY.observe_from_json(project_root, surface_json)
}

/// [`observe_from_json`] with the receipt serialized back to JSON.
pub fn observe_json(project_root: &Path, surface_json: &str) -> Result<String> {
    REGISTRY.observe_json(project_root, surface_json)
}

/// Forget a project root in the process-wide registry.
pub fn forget(project_root: &Path) -> bool {
    REGISTRY.forget(project_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, mpsc};
    use std::time::Duration;

    fn mirrored(surface: &EditorSurface) -> TmuxLayout {
        TmuxLayout {
            columns: surface.columns.clone(),
        }
    }

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
            force_reconcile: false,
        }
    }

    type Ran = Arc<Mutex<Vec<(PathBuf, SurfaceIntent)>>>;

    /// A registry whose consequence records instead of touching tmux.
    fn registry() -> (Registry, Ran) {
        let ran: Ran = Arc::new(Mutex::new(Vec::new()));
        let runner = {
            let ran = Arc::clone(&ran);
            move |root: &Path, intent: &SurfaceIntent| {
                ran.lock()
                    .unwrap()
                    .push((root.to_path_buf(), intent.clone()));
                Ok("ok".to_string())
            }
        };
        (Registry::new(Arc::new(runner)), ran)
    }

    /// The recording registry above, plus a tmux probe answering from `layouts`
    /// — one answer per editor observation, in order.
    fn registry_with_probe(layouts: Vec<Option<TmuxLayout>>) -> (Registry, Ran, Arc<Mutex<usize>>) {
        let ran: Ran = Arc::new(Mutex::new(Vec::new()));
        let runner = {
            let ran = Arc::clone(&ran);
            move |root: &Path, intent: &SurfaceIntent| {
                ran.lock()
                    .unwrap()
                    .push((root.to_path_buf(), intent.clone()));
                Ok("ok".to_string())
            }
        };
        let probes = Arc::new(Mutex::new(0usize));
        let probe = {
            let probes = Arc::clone(&probes);
            move |_: &Path, _: &EditorSurface| {
                let mut index = probes.lock().unwrap();
                let answer = layouts.get(*index).cloned().flatten();
                *index += 1;
                answer
            }
        };
        (
            Registry::with_tmux_probe(Arc::new(runner), Arc::new(probe)),
            ran,
            probes,
        )
    }

    #[test]
    fn a_pulled_tmux_layout_reconciles_drift_with_no_editor_change() {
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let drifted = TmuxLayout {
            columns: vec![SurfaceColumn::new(["/b.md"]), SurfaceColumn::new(["/a.md"])],
        };
        // The first observation already implies Sync and skips the probe.
        // The identical second surface probes and sees that tmux swapped panes.
        let (registry, ran, probes) = registry_with_probe(vec![Some(drifted)]);

        let first = registry.observe(Path::new("/p"), visible.clone());
        assert!(!first.idle, "the first sighting must reconcile the layout");

        let second = registry.observe(Path::new("/p"), visible.clone());
        assert!(
            !second.idle,
            "an unchanged editor surface must still reconcile once tmux is known to have drifted"
        );
        assert!(matches!(second.intent, SurfaceIntent::Sync { .. }));
        assert_eq!(
            *probes.lock().unwrap(),
            1,
            "only a repeated layout needs to pull the tmux mirror"
        );
        assert_eq!(ran.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_pulled_matching_layout_leaves_a_repeated_surface_idle() {
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let (registry, ran, _) = registry_with_probe(vec![Some(mirrored(&visible))]);

        registry.observe(Path::new("/p"), visible.clone());
        let second = registry.observe(Path::new("/p"), visible);

        assert!(second.idle, "a mirrored layout must not re-reconcile");
        assert_eq!(ran.lock().unwrap().len(), 1);
    }

    #[test]
    fn an_unanswered_probe_is_unknown_rather_than_drift() {
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        // `None`: the controller could not be asked for the repeated layout. An unreachable
        // controller must not read as drift, or every editor event reconciles.
        let (registry, ran, _) = registry_with_probe(vec![None]);

        registry.observe(Path::new("/p"), visible.clone());
        let second = registry.observe(Path::new("/p"), visible);

        assert!(second.idle);
        assert_eq!(ran.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_default_registry_never_probes() {
        let (registry, _) = registry();
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        registry.observe(Path::new("/p"), visible.clone());
        let second = registry.observe(Path::new("/p"), visible);
        assert!(
            second.idle,
            "without a probe the tmux side stays unobserved, so nothing drifts"
        );
    }

    #[test]
    fn an_inert_observation_costs_nothing() {
        let (registry, ran) = registry();
        let receipt = registry.observe(Path::new("/p"), EditorSurface::default());
        assert!(receipt.idle);
        assert_eq!(receipt.intent, SurfaceIntent::Idle);
        assert!(receipt.outcome.is_none() && receipt.error.is_none());
        assert!(
            ran.lock().unwrap().is_empty(),
            "no consequence may run for a surface that implies none"
        );
    }

    #[test]
    fn a_repeated_surface_runs_the_consequence_once() {
        let (registry, ran) = registry();
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);

        let first = registry.observe(Path::new("/p"), visible.clone());
        assert!(!first.idle, "the first sighting must reconcile the layout");
        assert_eq!(first.outcome.as_deref(), Some("ok"));

        let second = registry.observe(Path::new("/p"), visible);
        assert!(second.idle);
        assert!(second.outcome.is_none() && second.error.is_none());
        assert_eq!(
            ran.lock().unwrap().len(),
            1,
            "an unchanged surface must not re-run the consequence"
        );
    }

    #[test]
    fn a_focus_move_within_an_unchanged_layout_focuses_rather_than_reconciles() {
        let (registry, ran) = registry();
        registry.observe(Path::new("/p"), surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        registry.observe(Path::new("/p"), surface("/b.md", &[&["/a.md"], &["/b.md"]]));

        let ran = ran.lock().unwrap();
        assert_eq!(ran.len(), 2);
        assert!(matches!(ran[0].1, SurfaceIntent::Sync { .. }));
        assert_eq!(
            ran[1].1,
            SurfaceIntent::Focus {
                document: "/b.md".to_string()
            }
        );
    }

    #[test]
    fn the_consequence_receives_the_column_layout_it_must_reconcile() {
        let (registry, ran) = registry();
        registry.observe(Path::new("/p"), surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        let ran = ran.lock().unwrap();
        let SurfaceIntent::Sync { columns, document } = &ran[0].1 else {
            panic!("expected a sync intent, got {:?}", ran[0].1);
        };
        assert_eq!(document, "/a.md");
        assert_eq!(
            columns.iter().map(|c| c.files.clone()).collect::<Vec<_>>(),
            vec![vec!["/a.md".to_string()], vec!["/b.md".to_string()]]
        );
    }

    #[test]
    fn a_failing_consequence_is_reported_without_losing_the_observation() {
        let registry = Registry::new(Arc::new(|_: &Path, _: &SurfaceIntent| {
            Err(anyhow::anyhow!("no controller"))
        }));
        let visible = surface("/a.md", &[&["/a.md"]]);
        let receipt = registry.observe(Path::new("/p"), visible.clone());
        assert_eq!(receipt.error.as_deref(), Some("no controller"));
        assert!(receipt.outcome.is_none());

        let receipt = registry.observe(Path::new("/p"), visible);
        assert!(
            receipt.idle,
            "the observation is still recorded — a failed tmux command does not make it untrue"
        );
    }

    #[test]
    fn roots_are_independent() {
        let (registry, ran) = registry();
        let visible = surface("/a.md", &[&["/a.md"]]);

        assert!(!registry.observe(Path::new("/one"), visible.clone()).idle);
        assert!(
            !registry.observe(Path::new("/two"), visible.clone()).idle,
            "a second project root has its own history, not the first one's"
        );
        assert!(registry.observe(Path::new("/one"), visible).idle);

        let ran = ran.lock().unwrap();
        assert_eq!(ran.len(), 2);
        assert_eq!(ran[0].0, PathBuf::from("/one"));
        assert_eq!(ran[1].0, PathBuf::from("/two"));
    }

    #[test]
    fn forgetting_a_root_discards_its_history_and_stops_its_consequence() {
        let (registry, ran) = registry();
        let visible = surface("/a.md", &[&["/a.md"]]);
        assert!(!registry.observe(Path::new("/p"), visible.clone()).idle);
        assert!(registry.observe(Path::new("/p"), visible.clone()).idle);

        assert!(registry.forget(Path::new("/p")));
        assert!(
            !registry.observe(Path::new("/p"), visible).idle,
            "a reopened project starts from no reconciled layout"
        );
        assert_eq!(ran.lock().unwrap().len(), 2);
        assert!(!registry.forget(Path::new("/never-observed")));
    }

    #[test]
    fn a_controller_observed_drift_reconciles_without_an_editor_event() {
        let (registry, ran) = registry();
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        registry.observe_tmux(Path::new("/p"), Some(mirrored(&visible)));
        registry.observe(Path::new("/p"), visible);
        assert_eq!(ran.lock().unwrap().len(), 1);

        // Nothing arrives from the editor; the controller reports that the panes
        // no longer mirror the layout.
        let receipt = registry.observe_tmux(
            Path::new("/p"),
            Some(TmuxLayout {
                columns: vec![SurfaceColumn::new(["/b.md"]), SurfaceColumn::new(["/a.md"])],
            }),
        );
        assert!(!receipt.idle);
        assert!(matches!(receipt.intent, SurfaceIntent::Sync { .. }));
        assert_eq!(ran.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_tmux_observation_before_any_editor_report_is_inert() {
        let (registry, ran) = registry();
        let receipt = registry.observe_tmux(
            Path::new("/p"),
            Some(TmuxLayout {
                columns: vec![SurfaceColumn::new(["/a.md"])],
            }),
        );
        assert!(
            receipt.idle,
            "tmux state alone says nothing until the editor has reported a surface"
        );
        assert!(ran.lock().unwrap().is_empty());
    }

    #[test]
    fn json_round_trips_through_the_same_decision() {
        let (registry, _ran) = registry();
        let json = serde_json::to_string(&surface("/a.md", &[&["/a.md"], &["/b.md"]])).unwrap();

        let receipt: serde_json::Value =
            serde_json::from_str(&registry.observe_json(Path::new("/p"), &json).unwrap()).unwrap();
        assert_eq!(receipt["idle"], serde_json::json!(false));
        assert_eq!(receipt["intent"]["kind"], serde_json::json!("sync"));

        let receipt: serde_json::Value =
            serde_json::from_str(&registry.observe_json(Path::new("/p"), &json).unwrap()).unwrap();
        assert_eq!(receipt["idle"], serde_json::json!(true));
        assert_eq!(receipt["intent"]["kind"], serde_json::json!("idle"));

        assert!(registry.observe_json(Path::new("/p"), "not json").is_err());
    }

    #[test]
    fn deferred_surface_dispatch_is_non_blocking_and_latest_wins_per_root() {
        let (seen_tx, seen_rx) = mpsc::channel();
        let release_first = Arc::new((Mutex::new(false), Condvar::new()));
        let dispatcher = {
            let release_first = Arc::clone(&release_first);
            DeferredSurfaceDispatcher::new(Arc::new(move |_, surface| {
                seen_tx.send(surface.focused.clone()).unwrap();
                if surface.focused == "/one.md" {
                    let (released, wake) = &*release_first;
                    drop(
                        wake.wait_while(released.lock().unwrap(), |released| !*released)
                            .unwrap(),
                    );
                }
            }))
        };

        dispatcher
            .enqueue(PathBuf::from("/p"), surface("/one.md", &[&["/one.md"]]))
            .unwrap();
        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "/one.md"
        );

        dispatcher
            .enqueue(PathBuf::from("/p"), surface("/two.md", &[&["/two.md"]]))
            .unwrap();
        dispatcher
            .enqueue(PathBuf::from("/p"), surface("/three.md", &[&["/three.md"]]))
            .unwrap();

        let (released, wake) = &*release_first;
        *released.lock().unwrap() = true;
        wake.notify_all();

        assert_eq!(
            seen_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "/three.md"
        );
        assert!(
            seen_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "the superseded middle surface must never run",
        );
    }
}

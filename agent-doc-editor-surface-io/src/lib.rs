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

use agent_doc_editor_surface::{EditorSurface, EditorSurfaceState, SurfaceIntent};
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
    roots: Mutex<HashMap<PathBuf, RootSurface>>,
}

impl Registry {
    pub fn new(run_intent: IntentRunner) -> Self {
        Self {
            scope: ProcessScope::new(),
            run_intent,
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
    pub fn observe(&self, project_root: &Path, surface: EditorSurface) -> SurfaceObservationReceipt {
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
        entry.state.observe(surface);
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
                },
            )?;
            serde_json::to_string(&receipt).context("serialize sync receipt")
        }
    }
}

static REGISTRY: LazyLock<Registry> =
    LazyLock::new(|| Registry::new(Arc::new(run_intent_via_controller)));

/// Record an editor-surface observation for the process-wide registry.
pub fn observe(project_root: &Path, surface: EditorSurface) -> SurfaceObservationReceipt {
    REGISTRY.observe(project_root, surface)
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
    use agent_doc_editor_surface::SurfaceColumn;

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

    type Ran = Arc<Mutex<Vec<(PathBuf, SurfaceIntent)>>>;

    /// A registry whose consequence records instead of touching tmux.
    fn registry() -> (Registry, Ran) {
        let ran: Ran = Arc::new(Mutex::new(Vec::new()));
        let runner = {
            let ran = Arc::clone(&ran);
            move |root: &Path, intent: &SurfaceIntent| {
                ran.lock().unwrap().push((root.to_path_buf(), intent.clone()));
                Ok("ok".to_string())
            }
        };
        (Registry::new(Arc::new(runner)), ran)
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
}

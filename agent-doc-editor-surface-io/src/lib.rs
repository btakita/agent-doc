//! Editor-surface compatibility transport (`#jbpluginlazilyeffects`).
//!
//! [`agent_doc_editor_surface`] owns the pure fold from editor/tmux facts to an
//! intent. Runtime authority for that graph lives in the Project Controller.
//! This crate's production path only publishes ordered editor observations to
//! an already-running controller. It opens no durable store, starts no
//! controller, and creates no background runtime that can outlive a reloadable
//! native-library generation.
//!
//! [`Registry`] remains an ephemeral compatibility/test adapter for legacy FFI
//! callers. Current JetBrains and VS Code adapters bypass it: they publish
//! observations and read controller-owned projections over their own sockets.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

#[cfg(test)]
use agent_doc_editor_surface::SurfaceColumn;
use agent_doc_editor_surface::{
    CurrentDocumentAuthority, DocumentAuthority, EditorSurface, EditorSurfaceObservation,
    EditorSurfaceState, SurfaceIntent, TmuxLayout,
};
use agent_doc_state_scope::ProcessScope;
use anyhow::{Context as _, Result};

pub use agent_doc_editor_surface::SurfaceObservationReceipt;

/// Compatibility consequence for a locally derived intent.
pub type IntentRunner = Arc<dyn Fn(&Path, &SurfaceIntent) -> Result<String> + Send + Sync>;

/// Compatibility probe for the controller's half of the editor/tmux mirror.
///
/// Current adapters do not use this pull boundary. They publish the editor
/// Source to the controller-owned graph, where tmux is observed and reconciled.
/// `None` means "not asked, or no answer", deliberately distinct from a
/// matching or drifted layout.
pub type TmuxLayoutProbe = Arc<dyn Fn(&Path, &EditorSurface) -> Option<TmuxLayout> + Send + Sync>;

struct RootSurface {
    state: EditorSurfaceState,
    consequence: lazily::Effect,
    outcome: Arc<Mutex<Option<Result<String, String>>>>,
}

/// Ephemeral editor-surface graphs for one process, keyed by project root.
pub struct Registry {
    scope: ProcessScope,
    run_intent: IntentRunner,
    probe_tmux: TmuxLayoutProbe,
    roots: Mutex<HashMap<PathBuf, RootSurface>>,
}

impl Registry {
    /// A registry whose tmux Source is written only by [`Self::observe_tmux`].
    pub fn new(run_intent: IntentRunner) -> Self {
        Self::with_tmux_probe(run_intent, Arc::new(|_, _| None))
    }

    /// A compatibility registry that may pull one tmux observation after a
    /// repeated editor observation.
    pub fn with_tmux_probe(run_intent: IntentRunner, probe_tmux: TmuxLayoutProbe) -> Self {
        Self {
            scope: ProcessScope::new(),
            run_intent,
            probe_tmux,
            roots: Mutex::new(HashMap::new()),
        }
    }

    fn roots(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, RootSurface>> {
        // A panic in a consequence does not invalidate the already-committed
        // source cells, so recover the map instead of panicking into an editor.
        self.roots.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Record an editor surface and return the locally derived compatibility
    /// receipt.
    pub fn observe(
        &self,
        project_root: &Path,
        surface: EditorSurface,
    ) -> SurfaceObservationReceipt {
        let tmux = self
            .requires_tmux_probe(project_root, &surface)
            .then(|| (self.probe_tmux)(project_root, &surface))
            .flatten();
        self.with_entry(project_root, move |entry| {
            entry.state.observe_with_tmux(surface, tmux)
        })
    }

    /// Publish editor ingress without a compatibility tmux probe.
    fn observe_editor(
        &self,
        project_root: &Path,
        surface: EditorSurface,
    ) -> SurfaceObservationReceipt {
        self.with_entry(project_root, move |entry| entry.state.observe(surface))
    }

    fn requires_tmux_probe(&self, project_root: &Path, surface: &EditorSurface) -> bool {
        self.roots()
            .get(project_root)
            .is_some_and(|entry| entry.state.fold().tracking.requires_tmux_probe(surface))
    }

    /// Record a tmux-layout observation in the compatibility graph.
    pub fn observe_tmux(
        &self,
        project_root: &Path,
        layout: Option<TmuxLayout>,
    ) -> SurfaceObservationReceipt {
        self.with_entry(project_root, |entry| entry.state.observe_tmux(layout))
    }

    /// Record a controller-owned authority projection in the compatibility
    /// cache. Current editor adapters read this projection directly.
    pub fn observe_document_authority(&self, project_root: &Path, authority: DocumentAuthority) {
        let _ = self.with_entry(project_root, |entry| {
            entry.state.observe_document_authority(authority)
        });
    }

    pub fn document_authority(
        &self,
        project_root: &Path,
        document: &str,
    ) -> Option<DocumentAuthority> {
        let roots = self.roots();
        roots
            .get(project_root)
            .and_then(|entry| entry.state.document_authority(document))
            .or_else(|| {
                // A split editor surface can be rooted above the focused
                // document's nearest project root. Absolute identity is
                // unambiguous across these ephemeral cache entries.
                roots
                    .values()
                    .find_map(|entry| entry.state.document_authority(document))
            })
    }

    pub fn current_document_authority(&self, project_root: &Path) -> CurrentDocumentAuthority {
        self.roots()
            .get(project_root)
            .map(|entry| entry.state.current_document_authority())
            .unwrap_or_default()
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
                state.on_intent(move |intent| {
                    let result = run_intent(&root, intent).map_err(|error| format!("{error:#}"));
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

    pub fn observe_from_json(
        &self,
        project_root: &Path,
        surface_json: &str,
    ) -> Result<SurfaceObservationReceipt> {
        let surface: EditorSurface =
            serde_json::from_str(surface_json).context("parse editor surface json")?;
        Ok(self.observe(project_root, surface))
    }

    pub fn observe_json(&self, project_root: &Path, surface_json: &str) -> Result<String> {
        let receipt = self.observe_from_json(project_root, surface_json)?;
        serde_json::to_string(&receipt).context("serialize editor surface receipt")
    }

    /// Dispose a project's local compatibility graph and its Lazily effect.
    pub fn forget(&self, project_root: &Path) -> bool {
        let Some(entry) = self.roots().remove(project_root) else {
            return false;
        };
        entry.state.stop(&entry.consequence);
        true
    }

    /// Dispose every local compatibility graph owned by this native generation.
    pub fn forget_all(&self) -> usize {
        let entries = {
            let mut roots = self.roots();
            roots.drain().map(|(_, entry)| entry).collect::<Vec<_>>()
        };
        let root_count = entries.len();
        for entry in entries {
            entry.state.stop(&entry.consequence);
        }
        root_count
    }
}

static EDITOR_GENERATION_ACCEPTING: AtomicBool = AtomicBool::new(true);
static EDITOR_SURFACE_CLIENT_GENERATION: LazyLock<u64> = LazyLock::new(|| {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos().try_into().unwrap_or(u64::MAX))
        .unwrap_or(1)
});
static EDITOR_SURFACE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

// Ephemeral compatibility cache only. Intent authority and consequences live
// in the Project Controller; current editor adapters bypass this registry.
static REGISTRY: LazyLock<Registry> =
    LazyLock::new(|| Registry::new(Arc::new(|_, _| Ok(String::new()))));

fn publish_editor_observation(
    root: &Path,
    surface: EditorSurface,
) -> Result<SurfaceObservationReceipt> {
    let observation = EditorSurfaceObservation {
        client_id: format!("native-pid:{}", std::process::id()),
        generation: *EDITOR_SURFACE_CLIENT_GENERATION,
        sequence: EDITOR_SURFACE_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1,
        surface,
    };
    agent_doc_controller_io::project_controller::observe_editor_surface_existing(root, &observation)
}

/// Record an editor-surface observation through the process-wide compatibility
/// registry and the existing-controller socket.
pub fn observe(project_root: &Path, surface: EditorSurface) -> SurfaceObservationReceipt {
    if !EDITOR_GENERATION_ACCEPTING.load(Ordering::SeqCst) {
        return quiescing_receipt();
    }
    let _ = REGISTRY.observe_editor(project_root, surface.clone());
    publish_editor_observation(project_root, surface).unwrap_or_else(|error| {
        SurfaceObservationReceipt {
            intent: SurfaceIntent::Idle,
            idle: true,
            outcome: None,
            error: Some(format!("controller observation unavailable: {error:#}")),
        }
    })
}

fn quiescing_receipt() -> SurfaceObservationReceipt {
    SurfaceObservationReceipt {
        intent: SurfaceIntent::Idle,
        idle: true,
        outcome: None,
        error: Some("native editor generation is quiescing".to_string()),
    }
}

/// Validate and synchronously publish an editor-surface observation.
///
/// No background native task survives this compatibility call. Current editor
/// plugins publish over their own socket clients and do not use it.
pub fn enqueue_from_json(project_root: &Path, surface_json: &str) -> Result<()> {
    let surface: EditorSurface =
        serde_json::from_str(surface_json).context("parse editor surface json")?;
    anyhow::ensure!(
        EDITOR_GENERATION_ACCEPTING.load(Ordering::SeqCst),
        "native editor generation is quiescing"
    );
    let _ = REGISTRY.observe_editor(project_root, surface.clone());
    publish_editor_observation(project_root, surface)?;
    Ok(())
}

pub fn observe_tmux(project_root: &Path, layout: Option<TmuxLayout>) -> SurfaceObservationReceipt {
    if !EDITOR_GENERATION_ACCEPTING.load(Ordering::SeqCst) {
        return quiescing_receipt();
    }
    REGISTRY.observe_tmux(project_root, layout)
}

pub fn observe_from_json(
    project_root: &Path,
    surface_json: &str,
) -> Result<SurfaceObservationReceipt> {
    let surface: EditorSurface =
        serde_json::from_str(surface_json).context("parse editor surface json")?;
    Ok(observe(project_root, surface))
}

pub fn observe_json(project_root: &Path, surface_json: &str) -> Result<String> {
    let receipt = observe_from_json(project_root, surface_json)?;
    serde_json::to_string(&receipt).context("serialize editor surface receipt")
}

/// Retire the controller-owned client generation and dispose its local cache.
pub fn forget(project_root: &Path) -> bool {
    let client_id = format!("native-pid:{}", std::process::id());
    let controller_forgot =
        agent_doc_controller_io::project_controller::forget_editor_surface_existing(
            project_root,
            &client_id,
            *EDITOR_SURFACE_CLIENT_GENERATION,
        )
        .unwrap_or(false);
    REGISTRY.forget(project_root) || controller_forgot
}

/// Synchronously close ingress and dispose every local Lazily effect.
///
/// Automatic editor observations create no Tokio tasks, subscriptions, SQLite
/// mappings, or controller lifecycle work in this library generation.
pub fn quiesce_for_reload() {
    EDITOR_GENERATION_ACCEPTING.store(false, Ordering::SeqCst);
    let reactive_roots = REGISTRY.forget_all();
    eprintln!("[editor-surface] native generation quiesced reactive_roots={reactive_roots}");
}

/// Re-enable a retained generation after its replacement failed to load.
pub fn resume_after_reload_failure() {
    EDITOR_GENERATION_ACCEPTING.store(true, Ordering::SeqCst);
}

pub fn document_authority(project_root: &Path, document: &str) -> Option<DocumentAuthority> {
    REGISTRY.document_authority(project_root, document)
}

pub fn current_document_authority(project_root: &Path) -> CurrentDocumentAuthority {
    REGISTRY.current_document_authority(project_root)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    type Ran = Arc<Mutex<Vec<(PathBuf, SurfaceIntent)>>>;

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

    fn registry_with_probe(layout: Option<TmuxLayout>) -> (Registry, Ran) {
        let (base, ran) = registry();
        let run_intent = Arc::clone(&base.run_intent);
        (
            Registry::with_tmux_probe(run_intent, Arc::new(move |_, _| layout.clone())),
            ran,
        )
    }

    #[test]
    fn inert_and_repeated_surfaces_do_not_run_extra_consequences() {
        let (registry, ran) = registry();
        assert!(
            registry
                .observe(Path::new("/p"), EditorSurface::default())
                .idle
        );

        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        assert!(!registry.observe(Path::new("/p"), visible.clone()).idle);
        assert!(registry.observe(Path::new("/p"), visible).idle);
        assert_eq!(ran.lock().unwrap().len(), 1);
    }

    #[test]
    fn focus_move_in_an_unchanged_layout_derives_focus() {
        let (registry, ran) = registry();
        registry.observe(Path::new("/p"), surface("/a.md", &[&["/a.md"], &["/b.md"]]));
        registry.observe(Path::new("/p"), surface("/b.md", &[&["/a.md"], &["/b.md"]]));

        let ran = ran.lock().unwrap();
        assert!(matches!(ran[0].1, SurfaceIntent::Sync { .. }));
        assert_eq!(
            ran[1].1,
            SurfaceIntent::Focus {
                document: "/b.md".to_string()
            }
        );
    }

    #[test]
    fn pulled_tmux_drift_reconciles_a_repeated_surface() {
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        let drifted = TmuxLayout {
            columns: vec![SurfaceColumn::new(["/b.md"]), SurfaceColumn::new(["/a.md"])],
        };
        let (registry, ran) = registry_with_probe(Some(drifted));

        registry.observe(Path::new("/p"), visible.clone());
        let repeated = registry.observe(Path::new("/p"), visible);

        assert!(matches!(repeated.intent, SurfaceIntent::Sync { .. }));
        assert_eq!(ran.lock().unwrap().len(), 2);
    }

    #[test]
    fn matching_or_unanswered_probe_leaves_a_repeated_surface_idle() {
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        for layout in [Some(mirrored(&visible)), None] {
            let (registry, ran) = registry_with_probe(layout);
            registry.observe(Path::new("/p"), visible.clone());
            assert!(registry.observe(Path::new("/p"), visible.clone()).idle);
            assert_eq!(ran.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn consequence_failure_is_reported_without_losing_the_observation() {
        let registry = Registry::new(Arc::new(|_, _| Err(anyhow::anyhow!("no controller"))));
        let visible = surface("/a.md", &[&["/a.md"]]);

        let first = registry.observe(Path::new("/p"), visible.clone());
        assert_eq!(first.error.as_deref(), Some("no controller"));
        assert!(registry.observe(Path::new("/p"), visible).idle);
    }

    #[test]
    fn roots_are_independent_and_forget_discards_history() {
        let (registry, ran) = registry();
        let visible = surface("/a.md", &[&["/a.md"]]);

        assert!(!registry.observe(Path::new("/one"), visible.clone()).idle);
        assert!(!registry.observe(Path::new("/two"), visible.clone()).idle);
        assert!(registry.observe(Path::new("/one"), visible.clone()).idle);
        assert!(registry.forget(Path::new("/one")));
        assert!(!registry.observe(Path::new("/one"), visible).idle);
        assert_eq!(ran.lock().unwrap().len(), 3);
    }

    #[test]
    fn tmux_drift_alone_reconciles_after_editor_ingress() {
        let (registry, ran) = registry();
        let visible = surface("/a.md", &[&["/a.md"], &["/b.md"]]);
        registry.observe_tmux(Path::new("/p"), Some(mirrored(&visible)));
        registry.observe(Path::new("/p"), visible);

        let receipt = registry.observe_tmux(
            Path::new("/p"),
            Some(TmuxLayout {
                columns: vec![SurfaceColumn::new(["/b.md"]), SurfaceColumn::new(["/a.md"])],
            }),
        );
        assert!(matches!(receipt.intent, SurfaceIntent::Sync { .. }));
        assert_eq!(ran.lock().unwrap().len(), 2);
    }

    #[test]
    fn json_uses_the_same_graph_and_rejects_malformed_input() {
        let (registry, _) = registry();
        let json = serde_json::to_string(&surface("/a.md", &[&["/a.md"]])).unwrap();
        let receipt: serde_json::Value =
            serde_json::from_str(&registry.observe_json(Path::new("/p"), &json).unwrap()).unwrap();
        assert_eq!(receipt["intent"]["kind"], serde_json::json!("sync"));
        assert!(registry.observe_json(Path::new("/p"), "not json").is_err());
    }

    #[test]
    fn forget_all_disposes_every_local_effect() {
        let (registry, _) = registry();
        registry.observe(Path::new("/one"), surface("/a.md", &[&["/a.md"]]));
        registry.observe(Path::new("/two"), surface("/b.md", &[&["/b.md"]]));
        assert_eq!(registry.forget_all(), 2);
        assert!(registry.roots().is_empty());
        assert_eq!(registry.forget_all(), 0);
    }
}

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
    EditorSurfaceState, RemoteLayoutEvidence, RemoteLayoutMemory, RemoteLayoutResolution,
    SurfaceIntent, TmuxLayout,
};
use agent_doc_state_scope::ProcessScope;
use anyhow::{Context as _, Result};

pub use agent_doc_tmux_io::editor_view as editor_view_tmux;

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
            terminal_decision: Default::default(),
            deferred_to: None,
            rejected: None,
            reregister_above_generation: None,
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

// GH #134/#343: the last frontend-authoritative snapshot per project root.
// Legacy flat evidence can update focus but never creates or edits columns.
static REMOTE_LAYOUT_MEMORY: LazyLock<Mutex<HashMap<PathBuf, RemoteLayoutMemory>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn remote_layout_memory() -> std::sync::MutexGuard<'static, HashMap<PathBuf, RemoteLayoutMemory>> {
    REMOTE_LAYOUT_MEMORY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Resolve legacy Remote Dev focus evidence without deriving structure.
pub fn resolve_remote_layout(
    project_root: &Path,
    evidence: &RemoteLayoutEvidence,
) -> RemoteLayoutResolution {
    let mut memories = remote_layout_memory();
    let memory = memories.entry(project_root.to_path_buf()).or_default();
    let (next, resolution) = memory.advance(evidence);
    *memory = next;
    resolution
}

pub fn resolve_remote_layout_from_json(
    project_root: &Path,
    evidence_json: &str,
) -> Result<RemoteLayoutResolution> {
    let evidence: RemoteLayoutEvidence =
        serde_json::from_str(evidence_json).context("parse remote layout evidence json")?;
    Ok(resolve_remote_layout(project_root, &evidence))
}

/// Every editor derivation [`derive_editor_from_json`] answers (GH #326).
///
/// This is the hot-reloadable half of the plugin boundary: the plugin reports
/// evidence and the native library derives what it means. A derivation, once
/// shipped, is never removed or renamed and its input fields stay optional,
/// because after a native hot reload the library is newer than the plugin
/// generation still calling it.
pub const EDITOR_DERIVATIONS: &[&str] = &[
    "capabilities",
    "window_layout",
    "client_measured_layout",
    "surface_projection",
    "sync_layout",
    "remote_layout",
    "remote_client_surface",
    "registration_projection",
    "attach_failure_diagnosis",
    "route_outcome",
];

#[derive(serde::Deserialize)]
struct EditorDerivationRequest {
    derivation: String,
    #[serde(default)]
    input: serde_json::Value,
}

/// Answer one editor derivation request: `{"derivation": "<name>", "input": {...}}`
/// yields `{"derivation": "<name>", "output": {...}}`.
pub fn derive_editor_from_json(request_json: &str) -> Result<serde_json::Value> {
    let request: EditorDerivationRequest =
        serde_json::from_str(request_json).context("parse editor derivation request json")?;
    let input = if request.input.is_null() {
        serde_json::json!({})
    } else {
        request.input
    };
    let output = match request.derivation.as_str() {
        // GH #326 phase 5c: `captured_splice_inputs` lists the optional inputs
        // the captured-splice FFI calls accept (`registration`), so a plugin
        // sends one only to a library that shapes it natively.
        "capabilities" => serde_json::json!({
            "derivations": EDITOR_DERIVATIONS,
            "captured_splice_inputs": agent_doc_merge::captured_splice_request::CAPTURED_SPLICE_INPUTS,
        }),
        "window_layout" => {
            let evidence: agent_doc_editor_surface::WindowLayoutEvidence =
                serde_json::from_value(input).context("parse window layout evidence")?;
            serde_json::to_value(agent_doc_editor_surface::resolve_window_layout(&evidence))?
        }
        "client_measured_layout" => {
            let evidence: agent_doc_editor_surface::ClientMeasuredLayoutEvidence =
                serde_json::from_value(input).context("parse client measured layout evidence")?;
            serde_json::to_value(agent_doc_editor_surface::resolve_client_measured_layout(
                &evidence,
            ))?
        }
        "surface_projection" => {
            let evidence: agent_doc_editor_surface::SurfaceProjectionEvidence =
                serde_json::from_value(input).context("parse surface projection evidence")?;
            serde_json::to_value(agent_doc_editor_surface::project_surface(
                &evidence,
                |project_root, remote| resolve_remote_layout(Path::new(project_root), remote),
            ))?
        }
        "sync_layout" => {
            let evidence: agent_doc_editor_surface::SyncLayoutEvidence =
                serde_json::from_value(input).context("parse sync layout evidence")?;
            serde_json::to_value(agent_doc_editor_surface::derive_sync_layout(
                &evidence,
                nearest_agent_doc_root,
            ))?
        }
        "remote_layout" => {
            // GH #326 phase 4: Remote Dev compatibility observation (GH #134/#343)
            // through the derivation channel. It shares legacy compatibility state with the
            // pre-#326 `agent_doc_editor_surface_resolve_remote_layout_json`
            // symbol and `surface_projection`. Flat backend evidence is always non-structural.
            // The plugin's memoryless Kotlin fallback is retired: with no native answer the
            // Remote Dev layout is unknown.
            let evidence: agent_doc_editor_surface::RemoteSurfaceEvidence =
                serde_json::from_value(input).context("parse remote layout evidence")?;
            let observation = RemoteLayoutEvidence {
                clients: evidence.clients,
                focused: evidence.focused,
            };
            let root = Path::new(&evidence.project_root);
            let resolution = resolve_remote_layout(root, &observation);
            serde_json::to_value(resolution)?
        }
        "remote_client_surface" => {
            let evidence: agent_doc_editor_surface::RemoteClientSurfaceEvidence =
                serde_json::from_value(input).context("parse remote client surface evidence")?;
            serde_json::to_value(agent_doc_editor_surface::derive_remote_client_surface(
                &evidence,
                nearest_agent_doc_root,
            ))?
        }
        "registration_projection" => {
            // GH #326 phase 5: the CRDT replica registration direction
            // (`CrdtReplicaManager.forwarderFor`). The plugin reports presence
            // and equality facts about its three text generations, never the
            // text itself.
            let evidence: agent_doc_editor_surface::RegistrationProjectionEvidence =
                serde_json::from_value(input).context("parse registration projection evidence")?;
            serde_json::to_value(agent_doc_editor_surface::derive_registration_projection(
                &evidence,
            ))?
        }
        "attach_failure_diagnosis" => {
            // GH #326 phase 5b: what a CRDT replica attach failure reason means
            // for the operator (`CrdtReplicaManager.recordRegisterFailure`,
            // Compact Exchange): the remedy, the ops-log status token, whether
            // the hold is surfaced, whether liveness is republished, and the
            // notification text.
            let evidence: agent_doc_editor_surface::AttachFailureEvidence =
                serde_json::from_value(input).context("parse attach failure evidence")?;
            serde_json::to_value(agent_doc_editor_surface::diagnose_attach_failure(&evidence))?
        }
        "route_outcome" => {
            // `#jbshrinkrestarts`: what a Run Agent Doc route's output means
            // for the operator (`TerminalUtil.sendToTerminal`): the outcome
            // kind, the paused-queue and deferred-switch details, the
            // notification text, the snapshot path, and the success notices.
            let evidence: agent_doc_editor_surface::RouteOutcomeEvidence =
                serde_json::from_value(input).context("parse route outcome evidence")?;
            serde_json::to_value(agent_doc_editor_surface::derive_route_outcome(&evidence))?
        }
        other => anyhow::bail!(
            "unknown editor derivation `{other}`; this library answers {}",
            EDITOR_DERIVATIONS.join(", ")
        ),
    };
    Ok(serde_json::json!({ "derivation": request.derivation, "output": output }))
}

/// The nearest `.agent-doc` project root of the document at `file`, searched
/// from its directory upward (`TerminalUtil.nearestAgentDocProjectRoot`). The
/// path is made absolute and lexically normalized first, as the plugin did.
fn nearest_agent_doc_root(file: &str) -> Option<String> {
    use std::path::Component;
    let path = Path::new(file);
    let anchored = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in anchored.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let directory = normalized.parent()?;
    agent_doc_fs::find_project_root(directory).map(|root| root.to_string_lossy().into_owned())
}

fn publish_editor_observation(
    root: &Path,
    surface: EditorSurface,
) -> Result<SurfaceObservationReceipt> {
    let observation = EditorSurfaceObservation {
        client_id: format!("native-pid:{}", std::process::id()),
        generation: *EDITOR_SURFACE_CLIENT_GENERATION,
        sequence: EDITOR_SURFACE_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1,
        surface_id: "project".to_string(),
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
            terminal_decision: Default::default(),
            deferred_to: None,
            rejected: None,
            reregister_above_generation: None,
        }
    })
}

fn quiescing_receipt() -> SurfaceObservationReceipt {
    SurfaceObservationReceipt {
        intent: SurfaceIntent::Idle,
        idle: true,
        outcome: None,
        error: Some("native editor generation is quiescing".to_string()),
        terminal_decision: Default::default(),
        deferred_to: None,
        rejected: None,
        reregister_above_generation: None,
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
    remote_layout_memory().remove(project_root);
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
    remote_layout_memory().clear();
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

    /// GH #326: the native `window_layout` derivation answers every shared
    /// parity case. The plugin's frozen Kotlin floor answers the same file in
    /// `LayoutDetectorTest`, so the two cannot drift silently.
    #[test]
    fn gh326_window_layout_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/window_layout.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 8, "the parity file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let request =
                serde_json::json!({"derivation": "window_layout", "input": case["input"]});
            let answer = derive_editor_from_json(&request.to_string()).unwrap();
            let output = &answer["output"];
            let columns: Vec<serde_json::Value> = output["columns"]
                .as_array()
                .unwrap()
                .iter()
                .map(|column| column["files"].clone())
                .collect();
            let expected = &case["expected"];
            assert_eq!(
                serde_json::Value::Array(columns),
                expected["columns"],
                "{name}: columns"
            );
            assert_eq!(
                output["column_order"], expected["column_order"],
                "{name}: column_order"
            );
            assert_eq!(
                output.get("reason").cloned().unwrap_or_default(),
                expected["reason"],
                "{name}: reason"
            );
            assert_eq!(
                output.get("observed_line").cloned().unwrap_or_default(),
                expected["observed_line"],
                "{name}: observed_line"
            );
        }
    }

    /// GH #326 phase 2: the native `surface_projection` derivation answers every
    /// shared parity case. The plugin's frozen Kotlin floor answers the same file
    /// in `SurfaceProjectionTest`, so the two cannot drift silently.
    #[test]
    fn gh326_surface_projection_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/surface_projection.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 12, "the parity file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let evidence: agent_doc_editor_surface::SurfaceProjectionEvidence =
                serde_json::from_value(case["input"].clone()).unwrap();
            let mut resolved_remote = false;
            let projection =
                agent_doc_editor_surface::project_surface(&evidence, |root, remote| {
                    resolved_remote = true;
                    assert_eq!(root, "/repo", "{name}: remote observation key");
                    assert!(
                        !remote.clients.is_empty(),
                        "{name}: remote observation evidence"
                    );
                    serde_json::from_value(case["remote_resolution"].clone())
                        .unwrap_or_else(|error| panic!("{name}: remote_resolution: {error}"))
                });
            let output = serde_json::to_value(&projection).unwrap();
            let expected = &case["expected"];
            assert_eq!(output["outcome"], expected["outcome"], "{name}: outcome");
            assert_eq!(
                output.get("reason").cloned().unwrap_or_default(),
                expected["reason"],
                "{name}: reason"
            );
            assert_eq!(
                output.get("surface").cloned().unwrap_or_default(),
                expected["surface"],
                "{name}: surface"
            );
            assert_eq!(
                output["layout_source"], expected["layout_source"],
                "{name}: layout_source"
            );
            assert_eq!(
                resolved_remote,
                expected["layout_source"] == "remote",
                "{name}: Remote Dev resolution runs only for a ready remote projection"
            );
        }
    }

    /// GH #326 phase 2: the derivation channel answers `surface_projection` with
    /// the log lines the plugin used to compute and the Remote Dev compatibility
    /// resolution it runs.
    #[test]
    fn gh326_surface_projection_round_trips_through_the_derivation_channel() {
        let ready = derive_editor_from_json(
            r#"{"derivation":"surface_projection","input":{"base_path":"/repo","backend_window_count":2,
                "windows":[{"x":0,"y":0,"selected":"/repo/a.md","session_tabs":["/repo/a.md"]},
                           {"x":640,"y":0,"selected":"/repo/b.md","session_tabs":["/repo/b.md"]}],
                "anchor_selected_session":"/repo/a.md"}}"#,
        )
        .unwrap();
        let output = &ready["output"];
        assert_eq!(output["outcome"], "ready");
        assert_eq!(output["surface"]["focused"], "/repo/a.md");
        assert_eq!(output["surface"]["column_order"], "editor");
        assert_eq!(
            output["observed_line"],
            "[layout-detect] observed windows=2 snapshots=[(0,0) a.md, (640,0) b.md] columns=2 [a.md | b.md] source=windows"
        );

        let empty = derive_editor_from_json(r#"{"derivation":"surface_projection"}"#).unwrap();
        assert_eq!(empty["output"]["outcome"], "deferred");
        assert_eq!(
            empty["output"]["reason"],
            "backend_window_selection_incomplete windows=0 selected=0"
        );

        let unique_root = format!("/gh326-surface-projection-{}", std::process::id());
        let request = serde_json::json!({
            "derivation": "surface_projection",
            "input": {
                "base_path": unique_root,
                "manager_selected": [format!("{unique_root}/a.md")],
                "open_session_documents": [format!("{unique_root}/a.md")],
                "selected_text_editor_session": format!("{unique_root}/a.md"),
                "remote": {"project_root": unique_root, "clients": [
                    {"visible": ["a.md"], "selected": ["a.md"], "open": ["a.md"]}
                ], "focused": ["a.md"]}
            }
        });
        let remote = derive_editor_from_json(&request.to_string()).unwrap();
        let output = &remote["output"];
        assert_eq!(output["outcome"], "ready");
        assert_eq!(output["layout_source"], "remote");
        let line = output["observed_line"].as_str().unwrap();
        assert!(
            line.starts_with("[layout-detect] unknown windows=0 source=remote_client_selected_files focused=[a.md] remote_clients=1 selections=[0:[a.md]] visible=[0:[a.md]] reason="),
            "{line}"
        );
    }

    /// GH #326 phase 3: the native `sync_layout` derivation answers every
    /// shared parity case. The plugin's frozen Kotlin floor answers the same
    /// file in `SyncLayoutSnapshotTest`, so the two cannot drift silently.
    #[test]
    fn gh326_sync_layout_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/sync_layout.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 18, "the parity file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let evidence: agent_doc_editor_surface::SyncLayoutEvidence =
                serde_json::from_value(case["input"].clone()).unwrap();
            let roots = case["nearest_roots"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            let snapshot = agent_doc_editor_surface::derive_sync_layout(&evidence, |file| {
                roots
                    .get(file)
                    .and_then(|root| root.as_str())
                    .map(str::to_string)
            });
            let output = serde_json::to_value(&snapshot).unwrap();
            let expected = &case["expected"];
            for field in [
                "project_root",
                "layout",
                "columns",
                "column_order",
                "decision",
                "layout_mode",
                "candidate_roots",
            ] {
                assert_eq!(output[field], expected[field], "{name}: {field}");
            }
        }
    }

    /// GH #326 phase 3: the derivation channel answers `sync_layout`, resolving
    /// each visible document's nearest `.agent-doc` root on the filesystem.
    #[test]
    fn gh326_sync_layout_resolves_project_roots_through_the_derivation_channel() {
        let scratch =
            std::env::temp_dir().join(format!("gh326-sync-layout-{}", std::process::id()));
        let app = scratch.join("work/src/app");
        let other = scratch.join("work/src/other");
        for root in [&app, &other] {
            std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
            std::fs::create_dir_all(root.join("tasks")).unwrap();
        }
        let work = scratch.join("work").to_string_lossy().into_owned();
        let app = app.to_string_lossy().into_owned();
        let other = other.to_string_lossy().into_owned();
        let derive = |visible: Vec<String>| {
            let request = serde_json::json!({
                "derivation": "sync_layout",
                "input": {
                    "base_path": work,
                    "fallback_root": app,
                    "focused": format!("{app}/tasks/a.md"),
                    "visible": visible,
                    "layout": {"columns": [{"files": ["src/app/tasks/a.md"]}, {"files": ["src/app/tasks/./b.md"]}]}
                }
            });
            derive_editor_from_json(&request.to_string()).unwrap()["output"].clone()
        };
        let single = derive(vec![
            format!("{app}/tasks/a.md"),
            format!("{app}/tasks/../tasks/b.md"),
        ]);
        assert_eq!(single["project_root"], app.as_str());
        assert_eq!(
            single["columns"],
            serde_json::json!([format!("{app}/tasks/a.md"), format!("{app}/tasks/./b.md")])
        );
        assert_eq!(single["decision"], "publish");
        let cross = derive(vec![
            format!("{app}/tasks/a.md"),
            format!("{other}/tasks/c.md"),
        ]);
        assert_eq!(cross["project_root"], work.as_str());
        assert_eq!(cross["column_order"], "editor");
        assert_eq!(cross["layout_mode"], "exact");
        // GH #326 phase 3b: the tmux focus probe's candidates come from the
        // same filesystem reads.
        assert_eq!(
            cross["candidate_roots"],
            serde_json::json!([app, work, other])
        );
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    /// GH #326 phase 3b: Run Agent Doc's `--col` fallback passes the route's
    /// own root as `chosen_root`; the channel normalizes against it instead of
    /// the root that spans the visible layout.
    #[test]
    fn gh326_sync_layout_addresses_a_chosen_root_through_the_derivation_channel() {
        let scratch =
            std::env::temp_dir().join(format!("gh326-sync-chosen-{}", std::process::id()));
        let app = scratch.join("work/src/app");
        let other = scratch.join("work/src/other");
        for root in [&app, &other] {
            std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
            std::fs::create_dir_all(root.join("tasks")).unwrap();
        }
        let work = scratch.join("work").to_string_lossy().into_owned();
        let app = app.to_string_lossy().into_owned();
        let other = other.to_string_lossy().into_owned();
        let request = serde_json::json!({
            "derivation": "sync_layout",
            "input": {
                "base_path": work,
                "fallback_root": app,
                "chosen_root": app,
                "focused": format!("{app}/tasks/a.md"),
                "visible": [format!("{app}/tasks/a.md"), format!("{other}/tasks/c.md")],
                "layout": {"columns": [{"files": ["src/app/tasks/a.md"]}, {"files": ["src/other/tasks/c.md"]}]},
                "foreground": false
            }
        });
        let output = derive_editor_from_json(&request.to_string()).unwrap()["output"].clone();
        assert_eq!(output["project_root"], app.as_str());
        assert_eq!(
            output["columns"],
            serde_json::json!([format!("{app}/tasks/a.md"), format!("{other}/tasks/c.md")])
        );
        assert_eq!(output["layout_mode"], "exact");
        assert_eq!(output["decision"], "publish");
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    /// GH #326 phase 4: Remote Dev compatibility resolution answers every shared
    /// contract case in order from empty compatibility state with no controller authority.
    /// The plugin keeps no Kotlin copy of the rule (its memoryless fallback was
    /// retired); `LayoutDetectorTest` pins that it sends each step's input
    /// exactly and publishes each answer.
    #[test]
    fn gh326_remote_layout_answers_the_shared_contract_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/remote_layout.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 8, "the contract file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let mut memory = RemoteLayoutMemory::default();
            for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
                let evidence: agent_doc_editor_surface::RemoteSurfaceEvidence =
                    serde_json::from_value(step["input"].clone()).unwrap();
                let (next, resolution) = memory.advance(&RemoteLayoutEvidence {
                    clients: evidence.clients,
                    focused: evidence.focused,
                });
                memory = next;
                assert_eq!(
                    serde_json::to_value(&resolution).unwrap(),
                    step["expected"],
                    "{name}: step {index}"
                );
            }
        }
    }

    /// GH #326/#343: both compatibility entrypoints share focus history, but
    /// neither upgrades flat evidence into structural columns.
    #[test]
    fn gh326_remote_layout_entrypoints_share_focus_only_memory() {
        let root = format!("/tmp/gh326-remote-layout-{}", std::process::id());
        let split = serde_json::json!({
            "derivation": "remote_layout",
            "input": {
                "project_root": root,
                "clients": [{"visible": ["a.md", "b.md"], "selected": ["a.md"], "open": ["a.md", "b.md"]}],
                "focused": ["a.md"]
            }
        });
        let detected = derive_editor_from_json(&split.to_string()).unwrap();
        assert_eq!(detected["derivation"], "remote_layout");
        assert_eq!(detected["output"]["columns"], serde_json::json!([]));
        assert_eq!(detected["output"]["source"], "unknown");
        let single =
            r#"{"clients":[{"selected":["b.md"],"open":["a.md","b.md"]}],"focused":["b.md"]}"#;
        let retained = resolve_remote_layout_from_json(Path::new(&root), single).unwrap();
        assert!(retained.columns.is_empty());
        assert_eq!(retained.source.as_str(), "unknown");
        remote_layout_memory().remove(Path::new(&root));

        let empty = derive_editor_from_json(r#"{"derivation":"remote_layout"}"#).unwrap();
        assert_eq!(empty["output"]["columns"], serde_json::json!([]));
        assert_eq!(empty["output"]["source"], "unknown");
        remote_layout_memory().remove(Path::new(""));
    }

    /// GH #326 phase 5: the native `registration_projection` derivation
    /// answers every shared parity case through the derivation channel: the
    /// named cases and the exhaustive table. The plugin's frozen Kotlin floor
    /// answers the same file in `RegistrationProjectionTest`, so the two cannot
    /// drift silently.
    #[test]
    fn gh326_registration_projection_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/registration_projection.json"
        ))
        .unwrap();
        let derive = |input: serde_json::Value| {
            let request =
                serde_json::json!({"derivation": "registration_projection", "input": input});
            derive_editor_from_json(&request.to_string()).unwrap()["output"].clone()
        };
        // The facts the plugin captures from its three texts.
        let facts = |shadow: &serde_json::Value,
                     buffer: &serde_json::Value,
                     canonical: &serde_json::Value| {
            let both = |a: &serde_json::Value, b: &serde_json::Value| {
                a.as_str().is_some() && a.as_str() == b.as_str()
            };
            serde_json::json!({
                "has_published_shadow": shadow.is_string(),
                "has_buffer": buffer.is_string(),
                "has_canonical": canonical.is_string(),
                "buffer_equals_canonical": both(buffer, canonical),
                "buffer_equals_shadow": both(buffer, shadow),
                "canonical_equals_shadow": both(canonical, shadow),
            })
        };

        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 24, "the parity file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let texts = &case["texts"];
            let captured = facts(
                &texts["published_shadow"],
                &texts["buffer"],
                &texts["canonical"],
            );
            for (field, value) in captured.as_object().unwrap() {
                assert_eq!(&case["input"][field], value, "{name}: {field}");
            }
            assert_eq!(derive(case["input"].clone()), case["expected"], "{name}");
        }

        let table = &fixture["exhaustive"];
        let dimensions = table["dimensions"].as_array().unwrap();
        let answers: Vec<char> = table["answers"].as_str().unwrap().chars().collect();
        let combinations: usize = dimensions
            .iter()
            .map(|dimension| dimension["values"].as_array().unwrap().len())
            .product();
        assert_eq!(answers.len(), combinations, "one answer per combination");
        assert!(combinations >= 5184, "the exhaustive table lost dimensions");
        let text_dimensions = ["published_shadow", "buffer", "canonical"];
        for (index, code) in answers.iter().enumerate() {
            // Odometer: the last dimension varies fastest.
            let mut rest = index;
            let mut chosen = serde_json::Map::new();
            for dimension in dimensions.iter().rev() {
                let values = dimension["values"].as_array().unwrap();
                chosen.insert(
                    dimension["name"].as_str().unwrap().to_string(),
                    values[rest % values.len()].clone(),
                );
                rest /= values.len();
            }
            let mut input = serde_json::Map::new();
            for (name, value) in &chosen {
                if !text_dimensions.contains(&name.as_str()) {
                    input.insert(name.clone(), value.clone());
                }
            }
            let captured = facts(
                &chosen["published_shadow"],
                &chosen["buffer"],
                &chosen["canonical"],
            );
            input.extend(captured.as_object().unwrap().clone());
            let expected = &table["codes"][code.to_string()];
            assert!(expected.is_object(), "unknown answer code {code}");
            assert_eq!(
                &derive(serde_json::Value::Object(input)),
                expected,
                "combination {index}: {chosen:?}"
            );
        }
    }

    /// GH #326 phase 5b: the native `attach_failure_diagnosis` derivation
    /// answers every shared parity case through the derivation channel. The
    /// plugin's frozen Kotlin floor answers the same file in
    /// `AttachFailureDiagnosisTest`, so the two cannot drift silently.
    #[test]
    fn gh326_attach_failure_diagnosis_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/attach_failure_diagnosis.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 32, "the parity file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let request = serde_json::json!({
                "derivation": "attach_failure_diagnosis",
                "input": case["input"].clone(),
            });
            let output = derive_editor_from_json(&request.to_string()).unwrap()["output"].clone();
            assert_eq!(output, case["expected"], "{name}");
        }
    }

    /// `#jbshrinkrestarts`: the native `route_outcome` derivation answers every
    /// shared parity case through the derivation channel. The plugin's frozen
    /// Kotlin floor answers the same file in `RouteOutcomeTest`, so the two
    /// cannot drift silently.
    #[test]
    fn route_outcome_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/route_outcome.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 32, "the parity file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let request = serde_json::json!({
                "derivation": "route_outcome",
                "input": case["input"].clone(),
            });
            let output = derive_editor_from_json(&request.to_string()).unwrap()["output"].clone();
            assert_eq!(output, case["expected"], "{name}");
        }
        let listed = derive_editor_from_json(r#"{"derivation":"capabilities"}"#).unwrap();
        assert!(
            listed["output"]["derivations"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("route_outcome"))
        );
    }

    /// GH #326 phase 4: the native `remote_client_surface` derivation answers
    /// every shared parity case. The 262/263 backend's frozen Kotlin floor
    /// answers the same file in `RemoteClientMeasuredLayoutTest`, so the two
    /// cannot drift silently.
    #[test]
    fn gh326_remote_client_surface_answers_the_shared_parity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../editors/jetbrains/src/test/resources/editor-derivations/remote_client_surface.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 10, "the parity file lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let evidence: agent_doc_editor_surface::RemoteClientSurfaceEvidence =
                serde_json::from_value(case["input"].clone()).unwrap();
            let roots = case["nearest_roots"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            let forwarded =
                agent_doc_editor_surface::derive_remote_client_surface(&evidence, |file| {
                    roots
                        .get(file)
                        .and_then(|root| root.as_str())
                        .map(str::to_string)
                });
            assert_eq!(
                serde_json::to_value(&forwarded).unwrap(),
                case["expected"],
                "{name}"
            );
        }
    }

    /// GH #326 phase 4: the derivation channel answers `remote_client_surface`,
    /// resolving each document's nearest `.agent-doc` root on the filesystem.
    #[test]
    fn gh326_remote_client_surface_resolves_project_roots_through_the_derivation_channel() {
        let scratch = std::env::temp_dir().join(format!(
            "gh326-remote-client-surface-{}",
            std::process::id()
        ));
        let app = scratch.join("work/src/app");
        let other = scratch.join("work/src/other");
        for root in [&app, &other] {
            std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        }
        let work = scratch.join("work").to_string_lossy().into_owned();
        let app = app.to_string_lossy().into_owned();
        let other = other.to_string_lossy().into_owned();
        let derive = |columns: serde_json::Value| {
            let request = serde_json::json!({
                "derivation": "remote_client_surface",
                "input": {
                    "base_path": work,
                    "fallback_root": app,
                    "focused": format!("{app}/a.md"),
                    "columns": columns
                }
            });
            derive_editor_from_json(&request.to_string()).unwrap()["output"].clone()
        };
        let single = derive(serde_json::json!([
            [format!("{app}/b.md")],
            [format!("{app}/a.md")]
        ]));
        assert_eq!(single["project_root"], app.as_str());
        assert_eq!(single["surface"]["column_order"], "editor");
        assert_eq!(single["surface"]["preserve_focus"], true);
        assert_eq!(
            single["surface"]["columns"],
            serde_json::json!([{"files": [format!("{app}/b.md")]}, {"files": [format!("{app}/a.md")]}])
        );
        let cross = derive(serde_json::json!([
            [format!("{app}/a.md")],
            [format!("{other}/c.md")]
        ]));
        assert_eq!(cross["project_root"], work.as_str());
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    /// GH #326: the derivation channel answers every shipped derivation, names
    /// what it answers, and refuses an unknown one instead of guessing.
    #[test]
    fn gh326_editor_derivations_round_trip_through_json() {
        let layout = derive_editor_from_json(
            r#"{"derivation":"window_layout","input":{"base_path":"/repo","windows":[
                {"x":600,"y":0,"selected":"/repo/b.md","session_tabs":["/repo/b.md"]},
                {"x":0,"y":0,"selected":"/repo/a.md","session_tabs":["/repo/a.md"]}]}}"#,
        )
        .unwrap();
        assert_eq!(layout["derivation"], "window_layout");
        assert_eq!(
            layout["output"]["columns"],
            serde_json::json!([{"files": ["a.md"]}, {"files": ["b.md"]}])
        );
        assert_eq!(layout["output"]["column_order"], "editor");

        let measured = derive_editor_from_json(
            r#"{"derivation":"client_measured_layout","input":{"windows":[
                {"ordinal":0,"selected_session":"/r/a.md","holds_session_document":true,"x":0,"y":0},
                {"ordinal":1,"selected_session":"/r/b.md","holds_session_document":true,"x":0,"y":0}]}}"#,
        )
        .unwrap();
        assert_eq!(
            measured["output"],
            serde_json::json!({"outcome": "unavailable", "reason": "shared_origin"})
        );

        let empty = derive_editor_from_json(r#"{"derivation":"window_layout"}"#).unwrap();
        assert_eq!(empty["output"]["columns"], serde_json::json!([]));

        let capabilities = derive_editor_from_json(r#"{"derivation":"capabilities"}"#).unwrap();
        assert_eq!(
            capabilities["output"]["derivations"],
            serde_json::json!(EDITOR_DERIVATIONS)
        );

        let unknown = derive_editor_from_json(r#"{"derivation":"later_rule","input":{}}"#)
            .unwrap_err()
            .to_string();
        assert!(
            unknown.contains("unknown editor derivation `later_rule`"),
            "{unknown}"
        );
    }

    /// GH #343: flat legacy observations never invent structure, even when the
    /// same apparent visible set repeats for one project root.
    #[test]
    fn gh343_remote_layout_memory_never_promotes_repeated_flat_evidence() {
        let root = Path::new("/tmp/gh134-remote-layout-root");
        let other = Path::new("/tmp/gh134-remote-layout-other");
        let split = r#"{"clients":[{"visible":["a.md","b.md"],"selected":["a.md"],"open":["a.md","b.md"]}],"focused":["a.md"]}"#;
        for _ in 0..1_000 {
            assert!(
                resolve_remote_layout_from_json(root, split)
                    .unwrap()
                    .columns
                    .is_empty()
            );
        }
        assert!(
            resolve_remote_layout_from_json(other, split)
                .unwrap()
                .columns
                .is_empty()
        );
        remote_layout_memory().remove(root);
        remote_layout_memory().remove(other);
    }

    /// A cached exact frontend snapshot is preserved byte-for-byte across
    /// unknown, partial, reordered, and selection-only legacy evidence.
    #[test]
    fn gh343_flat_evidence_preserves_cached_frontend_snapshot() {
        let root = Path::new("/tmp/gh250-remote-layout-root");
        let exact = vec![
            SurfaceColumn::new(["right.md"]),
            SurfaceColumn::new(["left.md", "left-tab.md"]),
        ];
        remote_layout_memory().insert(
            root.to_path_buf(),
            RemoteLayoutMemory {
                columns: exact.clone(),
                focused: Some("right.md".to_string()),
            },
        );
        for observation in [
            r#"{}"#,
            r#"{"clients":[{"visible":["left.md"],"selected":["left.md"]}]}"#,
            r#"{"clients":[{"visible":["x.md","right.md","left.md"],"selected":["x.md"],"open":["x.md"]}]}"#,
        ] {
            assert_eq!(
                resolve_remote_layout_from_json(root, observation)
                    .unwrap()
                    .columns,
                exact
            );
        }
        remote_layout_memory().remove(root);
    }

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
            column_order: Default::default(),
            focused: focused.to_string(),
            open: visible.clone(),
            visible,
            columns,
            force_reconcile: false,
            focus_only: false,
            preserve_focus: false,
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

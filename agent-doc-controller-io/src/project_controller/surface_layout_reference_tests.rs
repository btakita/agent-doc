//! `surface_layout_reference_v1`: Run Agent Doc and Sync Tmux Panes name the
//! editor surface the controller already retains instead of sending a `--col`
//! snapshot (`specs/13-state-backbone.md` "State Propagation, Not Effect
//! Inference"). These tests drive the controller's real ingress: the editor
//! publishes through `editor_surface_observe`, then a command payload with a
//! `surface_reference` and no columns is resolved against that Source.

use super::*;
use agent_doc_editor_surface::{
    EditorSurface, EditorSurfaceObservation, FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY,
    SURFACE_LAYOUT_REFERENCE_CAPABILITY, SurfaceColumn, SurfaceColumnOrder,
};

const CLIENT: &str = "jetbrains@IU.test-pid:1";
const SURFACE: &str = "frame-1";

struct Fixture {
    _dir: tempfile::TempDir,
    bootstrap: ControllerBootstrap,
    runtime: Arc<ControllerRuntime>,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bootstrap = ControllerBootstrap {
            project_root: dir.path().to_path_buf(),
            socket_path: socket_path(dir.path()),
            launch_mode: LaunchMode::Lazy,
            bootstrap_epoch: 0,
            pid: std::process::id(),
            controller_binary: Some(current_binary_identity().unwrap()),
            controller_generation: 1,
            handoff_state: ControllerHandoffState::Stable,
            handoff_started_at: None,
            previous_controller_pid: None,
        };
        let runtime = ControllerRuntime::new_arc(bootstrap.clone()).unwrap();
        Self {
            _dir: dir,
            bootstrap,
            runtime,
        }
    }

    fn doc(&self, name: &str) -> String {
        self.bootstrap
            .project_root
            .join(name)
            .to_string_lossy()
            .to_string()
    }

    fn observe(&self, sequence: u64, surface: EditorSurface) -> serde_json::Value {
        let observation = EditorSurfaceObservation {
            client_id: CLIENT.to_string(),
            surface_id: SURFACE.to_string(),
            generation: 3,
            sequence,
            surface,
        };
        let mut request = empty_controller_request("editor_surface_observe");
        request.file = Some(PathBuf::from(&observation.surface.focused));
        request.generation = Some(observation.generation);
        request.caller = Some(CLIENT.to_string());
        request.diagnostic_payload = Some(serde_json::to_string(&observation).unwrap());
        let receipt =
            handle_editor_surface_observe(&self.bootstrap, self.runtime.as_ref(), request).unwrap();
        serde_json::to_value(SurfaceObservationReply::new(receipt)).unwrap()
    }

    fn sync_payload(&self, min_sequence: u64, no_autostart: bool) -> String {
        serde_json::json!({
            "project_root": self.bootstrap.project_root,
            "surface_reference": {"surface_id": SURFACE, "min_sequence": min_sequence},
            "no_autostart": no_autostart,
            "exact_visible": true,
            "caller_kind": "manual",
            "client_id": CLIENT,
        })
        .to_string()
    }

    fn resolve_sync(&self, payload: &str) -> Result<ControllerTmuxLayoutSyncInvocation> {
        sync_tmux_layout_invocation_from_payload(
            Some((&self.bootstrap, self.runtime.as_ref())),
            payload,
        )
    }
}

fn split(columns: &[&[&str]], focused: &str, order: SurfaceColumnOrder) -> EditorSurface {
    let visible = columns
        .iter()
        .flat_map(|column| column.iter().map(|file| file.to_string()))
        .collect::<Vec<_>>();
    EditorSurface {
        focused: focused.to_string(),
        open: visible.clone(),
        visible,
        columns: columns
            .iter()
            .map(|files| SurfaceColumn::new(files.iter().copied()))
            .collect(),
        column_order: order,
        force_reconcile: false,
        focus_only: false,
        preserve_focus: false,
    }
}

#[test]
fn surface_observe_reply_advertises_the_layout_reference_capability() {
    let fixture = Fixture::new();
    let a = fixture.doc("a.md");
    let reply = fixture.observe(1, split(&[&[&a]], &a, SurfaceColumnOrder::Editor));
    assert_eq!(
        reply["capabilities"],
        serde_json::json!([SURFACE_LAYOUT_REFERENCE_CAPABILITY]),
        "the reply an editor already reads must say the controller resolves references: {reply}"
    );
    assert!(
        reply.get("idle").is_some() && reply.get("intent").is_some(),
        "the receipt stays flattened into the reply, so older editors read it unchanged: {reply}"
    );
}

#[test]
fn sync_by_reference_derives_columns_from_the_current_surface_source() {
    let fixture = Fixture::new();
    let (a, b) = (fixture.doc("a.md"), fixture.doc("b.md"));
    fixture.observe(4, split(&[&[&a], &[&b]], &a, SurfaceColumnOrder::Editor));

    let invocation = fixture
        .resolve_sync(&fixture.sync_payload(4, false))
        .unwrap();
    assert_eq!(invocation.columns, vec![a.clone(), b.clone()]);
    assert_eq!(invocation.focus.as_deref(), Some(a.as_str()));
    assert_eq!(
        invocation.column_order,
        agent_doc_tmux::LayoutColumnOrder::Editor
    );
    assert_eq!(invocation.client_id.as_deref(), Some(CLIENT));
    assert!(!invocation.no_autostart);
    assert_eq!(invocation.caller_kind, "manual");

    // `[A | B]` then `[B | A]`: the current Source alone decides, with no event
    // history. The same reference resolves to the newer state.
    fixture.observe(5, split(&[&[&b], &[&a]], &b, SurfaceColumnOrder::Editor));
    let invocation = fixture
        .resolve_sync(&fixture.sync_payload(4, false))
        .unwrap();
    assert_eq!(invocation.columns, vec![b.clone(), a.clone()]);
    assert_eq!(invocation.focus.as_deref(), Some(b.as_str()));

    let ops_log = std::fs::read_to_string(
        fixture
            .bootstrap
            .project_root
            .join(".agent-doc/logs/ops.log"),
    )
    .unwrap_or_default();
    assert!(
        ops_log.contains("controller_sync_surface_reference_resolved")
            && ops_log.contains("sequence=5 min_sequence=4 columns=2"),
        "a resolved reference leaves an ops.log receipt naming the Source it read: {ops_log}"
    );
}

#[test]
fn sync_by_reference_keeps_a_remote_dev_order_label() {
    let fixture = Fixture::new();
    let (a, b) = (fixture.doc("a.md"), fixture.doc("b.md"));
    for (sequence, order, expected) in [
        (
            1,
            SurfaceColumnOrder::Unknown,
            agent_doc_tmux::LayoutColumnOrder::Unknown,
        ),
        (
            2,
            SurfaceColumnOrder::Retained,
            agent_doc_tmux::LayoutColumnOrder::Retained,
        ),
    ] {
        fixture.observe(sequence, split(&[&[&a], &[&b]], &a, order));
        let invocation = fixture
            .resolve_sync(&fixture.sync_payload(sequence, false))
            .unwrap();
        assert_eq!(invocation.column_order, expected);
    }
}

#[test]
fn sync_by_reference_fails_closed_without_a_usable_source() {
    let fixture = Fixture::new();
    let (a, b) = (fixture.doc("a.md"), fixture.doc("b.md"));

    let missing = fixture
        .resolve_sync(&fixture.sync_payload(1, false))
        .unwrap_err();
    assert!(
        format!("{missing:#}").contains("surface_reference_unavailable"),
        "{missing:#}"
    );

    fixture.observe(2, split(&[&[&a], &[&b]], &a, SurfaceColumnOrder::Editor));
    let stale = fixture
        .resolve_sync(&fixture.sync_payload(3, false))
        .unwrap_err();
    assert!(
        format!("{stale:#}").contains("surface_reference_stale"),
        "{stale:#}"
    );

    // An unknown split is published as `columns=[]`; a sync refuses it rather
    // than guessing columns from the visible set.
    let mut unknown = split(&[&[&a], &[&b]], &a, SurfaceColumnOrder::Editor);
    unknown.columns.clear();
    fixture.observe(4, unknown);
    let refused = fixture
        .resolve_sync(&fixture.sync_payload(4, false))
        .unwrap_err();
    let refused = format!("{refused:#}");
    assert!(
        refused.contains("surface_reference_unknown_layout: Remote Dev frontend structural routing is not ready (frontend_authority_absent_or_generation_mismatched; flat_evidence_has_no_column_geometry)"),
        "{refused}"
    );
    assert!(
        refused.contains(FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY),
        "{refused}"
    );
    assert!(
        refused.contains(
            "restarting only the Remote Dev backend cannot replace its already-loaded frontend module"
        ),
        "{refused}"
    );
    assert!(
        refused.contains(agent_doc_editor_surface::FRONTEND_AUTHORITY_UNAVAILABLE_READINESS),
        "{refused}"
    );
    assert!(refused.contains("then retry Sync Tmux Layout"), "{refused}");

    let no_client = serde_json::json!({
        "project_root": fixture.bootstrap.project_root,
        "surface_reference": {"surface_id": SURFACE, "min_sequence": 1},
        "exact_visible": true,
        "caller_kind": "manual",
    })
    .to_string();
    let refused = fixture.resolve_sync(&no_client).unwrap_err();
    assert!(
        format!("{refused:#}").contains("surface_reference_missing_client"),
        "{refused:#}"
    );
}

#[test]
fn sync_payload_without_columns_validates_only_with_a_surface_reference() {
    let fixture = Fixture::new();
    validate_async_editor_command_payload(
        AsyncEditorCommandKind::SyncTmuxLayout,
        &fixture.sync_payload(1, false),
    )
    .expect("a surface reference replaces the columns");
    let bare = serde_json::json!({
        "project_root": fixture.bootstrap.project_root,
        "caller_kind": "manual",
    })
    .to_string();
    assert!(
        validate_async_editor_command_payload(AsyncEditorCommandKind::SyncTmuxLayout, &bare)
            .is_err(),
        "a payload with neither columns nor a reference must still be refused"
    );
}

#[test]
fn explicit_col_publication_is_unchanged() {
    // The CLI (`agent-doc sync --col ...`) and an editor talking to this
    // controller without the capability keep the explicit `editor` publication.
    let fixture = Fixture::new();
    let payload = serde_json::json!({
        "columns": ["/p/a.md", "/p/b.md,/p/c.md"],
        "focus": "/p/a.md",
        "exact_visible": true,
        "caller_kind": "manual",
        "column_order": "editor",
    })
    .to_string();
    let resolved = fixture.resolve_sync(&payload).unwrap();
    let plain: ControllerTmuxLayoutSyncInvocation = serde_json::from_str(&payload).unwrap();
    assert_eq!(resolved, plain);
}

#[test]
fn route_by_reference_derives_layout_args_from_the_surface_source() {
    let fixture = Fixture::new();
    let (a, b) = (fixture.doc("a.md"), fixture.doc("b.md"));
    let canonical = fixture.bootstrap.project_root.join("a.md");
    let reference = agent_doc_editor_surface::SurfaceLayoutReference {
        surface_id: SURFACE.to_string(),
        min_sequence: 7,
    };
    let route_payload = |reference: &agent_doc_editor_surface::SurfaceLayoutReference| {
        let mut payload: ControllerEditorRoutePayload = serde_json::from_value(serde_json::json!({
            "relative_path": "a.md",
            "layout_args": [],
            "client_id": CLIENT,
            "surface_reference": reference,
        }))
        .unwrap();
        let reference = payload.surface_reference.take().unwrap();
        resolve_editor_route_surface_reference(
            &fixture.bootstrap,
            fixture.runtime.as_ref(),
            &canonical,
            &mut payload,
            &reference,
        )
        .map(|()| payload)
    };

    fixture.observe(7, split(&[&[&b], &[&a]], &a, SurfaceColumnOrder::Unknown));
    let payload = route_payload(&reference).unwrap();
    assert_eq!(
        payload.layout_args,
        vec!["--col", &b, "--col", &a, "--focus", &a]
    );
    assert_eq!(payload.layout_mode.as_deref(), Some("exact"));
    assert_eq!(
        payload.column_order,
        agent_doc_tmux::LayoutColumnOrder::Unknown
    );
    // The derived arguments are what the route validates and publishes.
    assert_eq!(
        validate_editor_route_layout_args(&payload.layout_args).unwrap(),
        payload.layout_args
    );

    // A single split only ensures each visible document has a column.
    fixture.observe(8, split(&[&[&a]], &a, SurfaceColumnOrder::Editor));
    let payload = route_payload(&reference).unwrap();
    assert_eq!(payload.layout_args, vec!["--col", &a, "--focus", &a]);
    assert_eq!(payload.layout_mode.as_deref(), Some("ensure"));
    assert_eq!(
        payload.column_order,
        agent_doc_tmux::LayoutColumnOrder::Unknown
    );

    let ahead = agent_doc_editor_surface::SurfaceLayoutReference {
        min_sequence: 9,
        ..reference
    };
    assert!(
        format!("{:#}", route_payload(&ahead).unwrap_err()).contains("surface_reference_stale")
    );
}

#[test]
fn legacy_route_payload_ignores_the_absent_reference() {
    let payload: ControllerEditorRoutePayload = serde_json::from_value(serde_json::json!({
        "relative_path": "a.md",
        "layout_args": ["--col", "/p/a.md"],
    }))
    .unwrap();
    assert!(payload.surface_reference.is_none());
    assert_eq!(payload.layout_args, vec!["--col", "/p/a.md"]);
}

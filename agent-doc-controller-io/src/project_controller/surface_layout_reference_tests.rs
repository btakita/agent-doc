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

    // An unknown split is published as `columns=[]`. A foreground manual sync
    // of a document with no started agent session bootstraps that one document
    // (GH #382) instead of guessing the visible set or refusing.
    let mut unknown = split(&[&[&a], &[&b]], &a, SurfaceColumnOrder::Editor);
    unknown.columns.clear();
    fixture.observe(4, unknown);
    let bootstrapped = fixture
        .resolve_sync(&fixture.sync_payload(4, false))
        .unwrap();
    assert_eq!(bootstrapped.columns, vec![a.clone()]);
    assert_eq!(bootstrapped.focus.as_deref(), Some(a.as_str()));
    assert_eq!(
        bootstrapped.column_order,
        agent_doc_tmux::LayoutColumnOrder::Unknown
    );
    assert!(bootstrapped.unstarted_session_bootstrap);
    assert!(bootstrapped.routes_created_panes());
    assert_ne!(bootstrapped.columns, vec![a.clone(), b.clone()]);

    let background = fixture
        .resolve_sync(&fixture.sync_payload(4, true))
        .unwrap_err();
    let background = format!("{background:#}");
    assert!(
        background.contains("surface_reference_unknown_layout: Remote Dev frontend structural routing is not ready (frontend_authority_absent_or_generation_mismatched; flat_evidence_has_no_column_geometry)"),
        "{background}"
    );
    assert!(
        background.contains(FRONTEND_AUTHORITY_UNAVAILABLE_REMEDY),
        "{background}"
    );
    assert!(
        background.contains(
            "restarting only the Remote Dev backend cannot replace its already-loaded frontend module"
        ),
        "{background}"
    );
    assert!(
        background.contains(agent_doc_editor_surface::FRONTEND_AUTHORITY_UNAVAILABLE_READINESS),
        "{background}"
    );
    assert!(
        background.contains("then retry Sync Tmux Layout"),
        "{background}"
    );

    let document_id = canonical_layout_document_id(&fixture.bootstrap.project_root, &a);
    fixture
        .runtime
        .actor_graph
        .set(std::collections::BTreeMap::from([(
            document_id,
            agent_doc_controller::actor::ActorRecord {
                document_id: canonical_layout_document_id(&fixture.bootstrap.project_root, &a),
                session_id: "session-%1".to_string(),
                generation: 7,
                pane_id: "%1".to_string(),
                window_id: "@1".to_string(),
                harness: "codex".to_string(),
                state: agent_doc_controller::actor::ActorState::Ready,
                last_transition: agent_doc_controller::actor::ActorLastTransition {
                    caller: "test".to_string(),
                    reason: "started".to_string(),
                    timestamp: 1,
                    prior_generation: 6,
                    new_generation: 7,
                },
            },
        )]));
    let started = fixture
        .resolve_sync(&fixture.sync_payload(4, false))
        .unwrap_err();
    let started = format!("{started:#}");
    assert!(
        started.contains("surface_reference_unknown_layout"),
        "{started}"
    );
    assert!(started.contains("then retry Sync Tmux Layout"), "{started}");

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

fn started_actor(
    document_id: &str,
    state: agent_doc_controller::actor::ActorState,
    pane_id: &str,
) -> agent_doc_controller::actor::ActorRecord {
    agent_doc_controller::actor::ActorRecord {
        document_id: document_id.to_string(),
        session_id: format!("session-{pane_id}"),
        generation: 7,
        pane_id: pane_id.to_string(),
        window_id: "@1".to_string(),
        harness: "codex".to_string(),
        state,
        last_transition: agent_doc_controller::actor::ActorLastTransition {
            caller: "test".to_string(),
            reason: "started".to_string(),
            timestamp: 1,
            prior_generation: 6,
            new_generation: 7,
        },
    }
}

fn manual_bootstrap_invocation(
    focus: &str,
    columns: Vec<String>,
) -> ControllerTmuxLayoutSyncInvocation {
    ControllerTmuxLayoutSyncInvocation {
        columns,
        window: None,
        focus: Some(focus.to_string()),
        no_autostart: false,
        exact_visible: false,
        caller_kind: "manual".to_string(),
        column_order: agent_doc_tmux::LayoutColumnOrder::Unknown,
        actor_bindings: Vec::new(),
        client_id: None,
        unstarted_session_bootstrap: true,
        layout_authority: None,
    }
}

#[test]
fn bootstrap_decision_accepts_only_one_unstarted_focus_column() {
    let root = std::path::PathBuf::from("/proj");
    let focus = "/proj/a.md";
    let mut invocation = manual_bootstrap_invocation(focus, vec![focus.to_string()]);
    assert!(matches!(
        unstarted_session_bootstrap_decision(&invocation, &root, false),
        UnstartedSessionBootstrapDecision::Apply
    ));
    assert!(matches!(
        unstarted_session_bootstrap_decision(&invocation, &root, true),
        UnstartedSessionBootstrapDecision::StartedSession
    ));
    invocation.columns = vec![focus.to_string(), "/proj/b.md".to_string()];
    assert!(matches!(
        unstarted_session_bootstrap_decision(&invocation, &root, false),
        UnstartedSessionBootstrapDecision::Ignore
    ));
    invocation.columns.clear();
    invocation.no_autostart = true;
    assert!(matches!(
        unstarted_session_bootstrap_decision(&invocation, &root, false),
        UnstartedSessionBootstrapDecision::Ignore
    ));
    invocation.unstarted_session_bootstrap = false;
    invocation.no_autostart = false;
    invocation.columns = vec![focus.to_string()];
    assert!(matches!(
        unstarted_session_bootstrap_decision(&invocation, &root, false),
        UnstartedSessionBootstrapDecision::Ignore
    ));
}

#[test]
fn closed_or_unbound_actor_is_not_a_started_agent_session() {
    let fixture = Fixture::new();
    let document = fixture.doc("a.md");
    let document_id = canonical_layout_document_id(&fixture.bootstrap.project_root, &document);
    let cases = [
        agent_doc_controller::actor::ActorState::Closed,
        agent_doc_controller::actor::ActorState::Ready,
    ];
    fixture
        .runtime
        .actor_graph
        .set(std::collections::BTreeMap::from([(
            document_id.clone(),
            started_actor(&document_id, cases[0], "%1"),
        )]));
    assert!(!document_has_started_agent_session(
        fixture.runtime.as_ref(),
        &fixture.bootstrap.project_root,
        &document,
    ));
    let mut unbound = started_actor(&document_id, cases[1], "");
    unbound.pane_id.clear();
    fixture
        .runtime
        .actor_graph
        .set(std::collections::BTreeMap::from([(
            document_id.clone(),
            unbound,
        )]));
    assert!(!document_has_started_agent_session(
        fixture.runtime.as_ref(),
        &fixture.bootstrap.project_root,
        &document,
    ));
    fixture
        .runtime
        .actor_graph
        .set(std::collections::BTreeMap::from([(
            document_id,
            started_actor(
                &canonical_layout_document_id(&fixture.bootstrap.project_root, &document),
                agent_doc_controller::actor::ActorState::Starting,
                "%2",
            ),
        )]));
    assert!(document_has_started_agent_session(
        fixture.runtime.as_ref(),
        &fixture.bootstrap.project_root,
        &document,
    ));
}

#[test]
fn unstarted_manual_sync_publishes_one_focus_column_without_a_retained_snapshot() {
    let fixture = Fixture::new();
    let path = fixture.bootstrap.project_root.join("a.md");
    std::fs::write(&path, "---\nagent_doc_session: s\n---\n# s\n").unwrap();
    let focus = path.canonicalize().unwrap().display().to_string();
    let (desired, applied) = publish_pane_layout_desired_invocation(
        &fixture.bootstrap,
        fixture.runtime.as_ref(),
        manual_bootstrap_invocation(&focus, vec![focus.clone()]),
        None,
        PaneLayoutPublication::FreshIntent,
        PaneLayoutPublisher::Command,
    )
    .unwrap();
    assert_eq!(applied.columns, vec![focus.clone()]);
    assert_eq!(desired.invocation.columns, vec![focus]);
    assert_eq!(
        applied.column_order,
        agent_doc_tmux::LayoutColumnOrder::Unknown
    );
}

#[test]
fn forged_bootstrap_flag_keeps_the_incomplete_frontend_deferral() {
    let fixture = Fixture::new();
    let error = publish_pane_layout_desired_invocation(
        &fixture.bootstrap,
        fixture.runtime.as_ref(),
        manual_bootstrap_invocation("/proj/a.md", vec!["/proj/a.md".into(), "/proj/b.md".into()]),
        None,
        PaneLayoutPublication::FreshIntent,
        PaneLayoutPublisher::Command,
    )
    .unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains("incomplete frontend layout has no authoritative snapshot"),
        "{error}"
    );
    assert!(error.contains("then retry Run Agent Doc"), "{error}");
    assert!(!error.contains("then retry Sync Tmux Layout"), "{error}");
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

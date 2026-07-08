//! Wire-contract tests for the lazily command-plane shadow endpoint
//! (`editor_command_submit`, `#lzmsgpcp`).
//!
//! These pin the exact `CommandSubmit` envelope the JetBrains / VS Code plugins
//! must send for `agent-doc.editor_route.v1`, and prove the terminal-only
//! resolution rule the controller relies on: progress events never complete the
//! command; only the terminal causal receipt does. The controller's
//! `handle_editor_command_submit_rpc` decodes exactly this shape and folds the
//! same projection.

use lazily::{
    CausalReceipt, CommandEvent, CommandEventKind, CommandEvents, CommandMessage, CommandPolicy,
    CommandProjection, CommandStatus, CommandSubmit, DedupePolicy, IpcValue,
};

/// The `agent-doc.editor_route.v1` payload body a plugin serializes into
/// `CommandSubmit.payload` (inline UTF-8 JSON) — the exact shape
/// `handle_editor_route_rpc` already consumes as `ControllerEditorRoutePayload`.
fn editor_route_payload_json() -> String {
    serde_json::json!({
        "relative_path": "plan.md",
        "layout_args": [],
        "dispatch_only": true,
        "plain_trigger": true,
        "wait_for_ready_secs": 15,
        "force_disk": false,
        "attempt_id": "attempt-7",
        "route_key": "project-root:plan.md:run",
        "source": "vscode-plugin"
    })
    .to_string()
}

fn editor_route_submit(command_id: &str, generation: u64) -> CommandSubmit {
    command_submit(
        command_id,
        generation,
        "editor_route",
        "agent-doc.editor_route.v1",
        "project-root:plan.md:run",
        editor_route_payload_json(),
    )
}

fn command_submit(
    command_id: &str,
    generation: u64,
    name: &str,
    payload_type: &str,
    idempotency_key: &str,
    payload_json: String,
) -> CommandSubmit {
    CommandSubmit {
        command_id: command_id.to_string(),
        causation_id: command_id.to_string(),
        source: "vscode-plugin".to_string(),
        target: "project-controller".to_string(),
        namespace: "agent-doc".to_string(),
        name: name.to_string(),
        authority_generation: generation,
        idempotency_key: idempotency_key.to_string(),
        deadline_ms: 120_000,
        policy: CommandPolicy {
            dedupe: DedupePolicy::SameIdempotencyKey,
            supersede: false,
            cancel_on_preempt: true,
        },
        payload_type: payload_type.to_string(),
        payload_hash: "sha256:contract".to_string(),
        payload: IpcValue::Inline(payload_json.into_bytes()),
        required_features: vec!["causal-receipts".to_string(), "command-events".to_string()],
    }
}

#[test]
fn submit_envelope_round_trips_and_exposes_editor_route_payload() {
    // A plugin sends this over the wire; the controller decodes it exactly.
    let message = CommandMessage::CommandSubmit(Box::new(editor_route_submit("cmd-run-1", 42)));
    let wire = serde_json::to_string(&message).unwrap();
    let decoded: CommandMessage = serde_json::from_str(&wire).unwrap();

    let CommandMessage::CommandSubmit(submit) = decoded else {
        panic!("expected CommandSubmit");
    };
    assert_eq!(submit.namespace, "agent-doc");
    assert_eq!(submit.name, "editor_route");
    assert!(submit.payload_type.starts_with("agent-doc.editor_route."));

    // The inline payload extracts to the editor_route JSON the existing handler consumes.
    let IpcValue::Inline(bytes) = &submit.payload else {
        panic!("expected inline payload");
    };
    let payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(payload["relative_path"], "plan.md");
    assert_eq!(payload["dispatch_only"], true);
    assert_eq!(payload["route_key"], "project-root:plan.md:run");
}

#[test]
fn submit_envelope_round_trips_and_exposes_sync_tmux_layout_payload() {
    let payload_json = serde_json::json!({
        "project_root": "/repo",
        "columns": ["/repo/tasks/one.md,/repo/tasks/two.md"],
        "focus": "/repo/tasks/two.md",
        "no_autostart": false,
        "exact_visible": true,
        "caller_kind": "manual"
    })
    .to_string();
    let submit = command_submit(
        "cmd-sync-1",
        7,
        "sync_tmux_layout",
        "agent-doc.sync_tmux_layout.v1",
        "/repo:sync",
        payload_json,
    );
    let message = CommandMessage::CommandSubmit(Box::new(submit));
    let decoded: CommandMessage =
        serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap();
    let CommandMessage::CommandSubmit(submit) = decoded else {
        panic!("expected CommandSubmit");
    };
    assert_eq!(submit.name, "sync_tmux_layout");
    assert_eq!(submit.payload_type, "agent-doc.sync_tmux_layout.v1");
    assert_eq!(submit.idempotency_key, "/repo:sync");

    let IpcValue::Inline(bytes) = &submit.payload else {
        panic!("expected inline payload");
    };
    let payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(
        payload["columns"][0],
        "/repo/tasks/one.md,/repo/tasks/two.md"
    );
    assert_eq!(payload["focus"], "/repo/tasks/two.md");
    assert_eq!(payload["exact_visible"], true);
}

#[test]
fn submit_envelope_round_trips_and_exposes_focus_document_pane_payload() {
    let payload_json = serde_json::json!({
        "project_root": "/repo",
        "document_path": "/repo/tasks/one.md",
        "no_promotion": true,
        "active_window_guard": true
    })
    .to_string();
    let submit = command_submit(
        "cmd-focus-1",
        9,
        "focus_document_pane",
        "agent-doc.focus_document_pane.v1",
        "/repo:/repo/tasks/one.md:focus",
        payload_json,
    );
    let message = CommandMessage::CommandSubmit(Box::new(submit));
    let decoded: CommandMessage =
        serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap();
    let CommandMessage::CommandSubmit(submit) = decoded else {
        panic!("expected CommandSubmit");
    };
    assert_eq!(submit.name, "focus_document_pane");
    assert_eq!(submit.payload_type, "agent-doc.focus_document_pane.v1");

    let IpcValue::Inline(bytes) = &submit.payload else {
        panic!("expected inline payload");
    };
    let payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(payload["document_path"], "/repo/tasks/one.md");
    assert_eq!(payload["active_window_guard"], true);
}

#[test]
fn progress_events_never_complete_the_command() {
    // Mirrors the controller's projection fold before the route settles.
    let submit = editor_route_submit("cmd-run-1", 42);
    let mut projection = CommandProjection::new();
    projection.submit(&submit);
    projection.apply_message(&CommandMessage::CommandEvents(CommandEvents {
        events: vec![
            CommandEvent {
                event_id: "cmd-run-1-observed".into(),
                command_id: "cmd-run-1".into(),
                kind: CommandEventKind::Observed,
                generation: 42,
                detail: None,
            },
            CommandEvent {
                event_id: "cmd-run-1-accepted".into(),
                command_id: "cmd-run-1".into(),
                kind: CommandEventKind::Accepted,
                generation: 42,
                detail: None,
            },
            CommandEvent {
                event_id: "cmd-run-1-started".into(),
                command_id: "cmd-run-1".into(),
                kind: CommandEventKind::Started,
                generation: 42,
                detail: None,
            },
        ],
    }));
    // Accepted/started progress is not terminal — the RPC call must keep waiting.
    assert!(projection.terminal_for("cmd-run-1").is_none());
}

#[test]
fn applied_receipt_makes_editor_route_terminal() {
    let submit = editor_route_submit("cmd-run-1", 42);
    let mut projection = CommandProjection::new();
    projection.submit(&submit);
    // Route exit_code == 0 → applied terminal receipt.
    let receipt =
        lazily::applied_receipt("cmd-run-1-receipt", "cmd-run-1", "project-controller", 42);
    projection.observe_receipt(&receipt);

    let entry = projection.terminal_for("cmd-run-1").expect("terminal");
    assert_eq!(entry.status, CommandStatus::Applied);
    assert_eq!(
        entry.terminal_receipt_id.as_deref(),
        Some("cmd-run-1-receipt")
    );
}

#[test]
fn route_failure_is_a_terminal_rejected_receipt_not_an_rpc_error() {
    let submit = editor_route_submit("cmd-run-1", 42);
    let mut projection = CommandProjection::new();
    projection.submit(&submit);
    // Route exit_code != 0 → rejected terminal receipt (still terminal proof).
    let receipt: CausalReceipt = lazily::rejected_receipt(
        "cmd-run-1-receipt",
        "cmd-run-1",
        "project-controller",
        42,
        "editor_route exit_code=1",
    );
    projection.observe_receipt(&receipt);

    let entry = projection.terminal_for("cmd-run-1").expect("terminal");
    assert_eq!(entry.status, CommandStatus::Rejected);
    assert_eq!(entry.reason.as_deref(), Some("editor_route exit_code=1"));
}

//! Realtime selected-text steering for an already-active authoritative turn.

use agent_doc_controller_io::project_controller::{
    ControllerTurnSteeringOutcome, ControllerTurnSteeringReceipt,
};
use agent_doc_supervisor::ipc_protocol::IpcMethod;
use anyhow::{Context, Result};
use std::path::Path;

pub fn deliver_active_turn_steering(
    file: &Path,
    steering_id: &str,
    text: &str,
) -> Result<ControllerTurnSteeringReceipt> {
    if steering_id.trim().is_empty() {
        anyhow::bail!("turn steering requires a non-empty steering id");
    }
    let session_id = agent_doc_frontmatter_io::session::read_session_id(file)
        .with_context(|| format!("{} has no agent_doc_session", file.display()))?;
    let socket = crate::supervisor_runtime::supervisor_socket_path(file, &session_id)
        .with_context(|| format!("cannot resolve supervisor socket for {}", file.display()))?;

    let state = agent_doc_supervisor_io::ipc::send_command(&socket, &IpcMethod::State)
        .with_context(|| {
            format!(
                "failed to query authoritative supervisor for {}",
                file.display()
            )
        })?;
    if !state.ok {
        anyhow::bail!(
            "authoritative supervisor state query failed for {}: {}",
            file.display(),
            state.error.as_deref().unwrap_or("unknown supervisor error")
        );
    }
    let state = state
        .data
        .context("authoritative supervisor state response omitted data")?;
    let actor_state = state
        .get("actor_state")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let actor_session_id = state
        .get("actor_session_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if actor_state != "busy" || actor_session_id != session_id {
        anyhow::bail!(
            "selected-text turn steering requires the authoritative pane to have an active turn (actor_state={actor_state:?}, actor_session={actor_session_id:?})"
        );
    }
    let actor_pane_id = state
        .get("actor_pane_id")
        .and_then(serde_json::Value::as_str)
        .context("authoritative supervisor state omitted actor_pane_id")?
        .to_string();
    let actor_generation = state
        .get("actor_generation")
        .and_then(serde_json::Value::as_u64)
        .context("authoritative supervisor state omitted actor_generation")?;

    let response = agent_doc_supervisor_io::ipc::send_command(
        &socket,
        &IpcMethod::Steer {
            steering_id: steering_id.to_string(),
            bytes: text.to_string(),
        },
    )
    .with_context(|| {
        format!(
            "failed to deliver selected-text steering for {}",
            file.display()
        )
    })?;
    if !response.ok {
        anyhow::bail!(
            "authoritative supervisor rejected selected-text steering for {}: {}",
            file.display(),
            response
                .error
                .as_deref()
                .unwrap_or("unknown supervisor error")
        );
    }
    let ack = response
        .data
        .context("turn steering response omitted acknowledgement data")?;
    if ack.get("kind").and_then(serde_json::Value::as_str) != Some("turn_steering_ack") {
        anyhow::bail!("turn steering response had an unexpected acknowledgement kind");
    }
    let ack_id = ack
        .get("steering_id")
        .and_then(serde_json::Value::as_str)
        .context("turn steering acknowledgement omitted steering_id")?;
    if ack_id != steering_id {
        anyhow::bail!(
            "turn steering acknowledgement id mismatch: expected {steering_id}, got {ack_id}"
        );
    }
    let outcome = match ack
        .get("outcome")
        .and_then(serde_json::Value::as_str)
        .context("turn steering acknowledgement omitted outcome")?
    {
        "delivered" => ControllerTurnSteeringOutcome::Delivered,
        "duplicate" => ControllerTurnSteeringOutcome::Duplicate,
        other => anyhow::bail!("unknown turn steering acknowledgement outcome: {other}"),
    };
    let accepted_bytes = ack
        .get("n")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .context("turn steering acknowledgement omitted a valid byte count")?;

    Ok(ControllerTurnSteeringReceipt {
        kind: "turn_steering_ack".to_string(),
        steering_id: steering_id.to_string(),
        outcome,
        accepted_bytes,
        actor_session_id,
        actor_pane_id,
        actor_generation,
    })
}

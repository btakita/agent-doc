//! Realtime selected-text steering for an already-active authoritative turn.

use agent_doc_controller_io::project_controller::{
    ControllerTurnSteeringOutcome, ControllerTurnSteeringReceipt,
};
use agent_doc_supervisor::ipc_protocol::IpcMethod;
use anyhow::{Context, Result};
use std::path::Path;

/// Whether an actor in `actor_state` can receive steered text at all.
///
/// `#steergateidle` — an **active turn is not a precondition**. This gate used to
/// require `actor_state == "busy"`, so a selection sent at an idle pane was rejected
/// with an unsatisfiable instruction: there was no turn to steer and no way for the
/// operator to create one from the editor. Operator directive 2026-07-25: idle must
/// deliver rather than error. A live harness is waiting for input in exactly that
/// state, which is the case where delivering the selection is most obviously right.
///
/// Dropping the requirement also stops the supervisor's state classification from
/// being load-bearing. A turn that WAS active but still read `"ready"` (observed
/// 2026-07-25, alongside repeated 5s controller-response timeouts on the same actor)
/// previously turned a state-tracking lag into a hard user-visible failure. Both live
/// states now accept, so that lag degrades to a no-op instead of a rejection.
///
/// What still fails closed is a pane with nothing able to receive input — `starting`
/// (no composer yet), `closed`, `blocked`, or a state the supervisor could not report
/// (`missing`/absent, which arrives here as an empty string). The caller additionally
/// requires the actor session to match, which is the real safety property: never
/// inject one document's selection into another session's pane.
fn actor_state_accepts_steering(actor_state: &str) -> bool {
    matches!(actor_state, "busy" | "ready" | "waiting_input")
}

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
    if !actor_state_accepts_steering(actor_state) || actor_session_id != session_id {
        anyhow::bail!(
            "selected-text turn steering needs the authoritative pane to be able to accept input for this session (actor_state={actor_state:?}, actor_session={actor_session_id:?})"
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `#steergateidle` — the operator-reported regression: a selection sent at an
    /// idle pane was rejected because the gate demanded an active turn, leaving the
    /// operator an instruction they could not satisfy (there was no turn to steer,
    /// and the editor could not create one).
    ///
    /// The second half of that report matters just as much: a turn that WAS active
    /// still read `"ready"`. Accepting both live states is what stops a supervisor
    /// state-tracking lag from becoming a hard user-visible failure — the whole
    /// point is that this predicate must not be load-bearing on that distinction.
    #[test]
    fn steering_does_not_require_an_active_turn() {
        assert!(
            actor_state_accepts_steering("ready"),
            "an idle pane must accept a selection instead of rejecting it"
        );
        assert!(
            actor_state_accepts_steering("busy"),
            "an active turn must still accept steering"
        );
        assert!(
            actor_state_accepts_steering("waiting_input"),
            "a pane explicitly waiting for input is the clearest accept case"
        );
    }

    /// A pane with nothing able to receive input still fails closed — the gate is
    /// narrowed, not removed. `""` is the shape an unreportable state arrives in.
    #[test]
    fn steering_still_fails_closed_when_the_pane_cannot_receive_input() {
        for state in ["starting", "closed", "blocked", "missing", ""] {
            assert!(
                !actor_state_accepts_steering(state),
                "{state:?} has no composer able to receive steered text"
            );
        }
    }
}

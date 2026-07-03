use std::path::Path;

use agent_doc_document_realtime::crdt_relay::AwarenessState;
use agent_doc_supervisor::ipc_protocol::IpcResponse;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

/// CRDT live multi-editor delta fan-out IPC handlers (`#crdtauth5`).
///
/// Each handler routes the editor-replica IPC family through the per-document
/// `agent-doc-crdt-relay-io` hub registry. The hub-host functions resolve the
/// document's authority first and refuse when the document has no live editor,
/// so this family is inert on the headless control-plane path.
pub fn handle_replica_register(file: &str, identity: &str) -> IpcResponse {
    match agent_doc_crdt_relay_io::register_replica_for_file(Path::new(file), identity) {
        Ok(Some((client_id, bootstrap))) => IpcResponse::ok(serde_json::json!({
            "client_id": client_id,
            "bootstrap_b64": BASE64_STANDARD.encode(&bootstrap),
        })),
        Ok(None) => {
            IpcResponse::err("crdt replica register refused: document is not editor-attached")
        }
        Err(e) => IpcResponse::err(format!("crdt replica register failed: {e}")),
    }
}

pub fn handle_replica_deregister(file: &str, identity: &str) -> IpcResponse {
    match agent_doc_crdt_relay_io::deregister_replica_for_file(Path::new(file), identity) {
        Ok(removed) => IpcResponse::ok(serde_json::json!({ "removed": removed })),
        Err(e) => IpcResponse::err(format!("crdt replica deregister failed: {e}")),
    }
}

pub fn handle_replica_update(file: &str, identity: &str, update_b64: &str) -> IpcResponse {
    let update = match BASE64_STANDARD.decode(update_b64) {
        Ok(bytes) => bytes,
        Err(e) => return IpcResponse::err(format!("crdt replica update: bad base64: {e}")),
    };
    match agent_doc_crdt_relay_io::relay_replica_update_for_file(Path::new(file), identity, &update)
    {
        Ok(Some(fan_out)) => {
            let targets: Vec<serde_json::Value> = fan_out
                .targets
                .iter()
                .map(|target| {
                    serde_json::json!({
                        "client_id": target,
                        "update_b64": BASE64_STANDARD.encode(&fan_out.update),
                    })
                })
                .collect();
            IpcResponse::ok(serde_json::json!({
                "origin": fan_out.origin,
                "canonical_len": fan_out.canonical_len,
                "targets": targets,
            }))
        }
        Ok(None) => {
            IpcResponse::err("crdt replica update refused: document is not editor-attached")
        }
        Err(e) => IpcResponse::err(format!("crdt replica update failed: {e}")),
    }
}

pub fn handle_replica_pull(file: &str, identity: &str) -> IpcResponse {
    match agent_doc_crdt_relay_io::pull_replica_updates_for_file(Path::new(file), identity) {
        Ok(Some(pull)) => {
            let updates: Vec<serde_json::Value> = pull
                .updates
                .iter()
                .map(|update| {
                    serde_json::json!({
                        "patch_id": update.patch_id,
                        "origin": update.origin,
                        "target": update.target,
                        "generation": update.generation,
                        "update_b64": BASE64_STANDARD.encode(&update.update),
                    })
                })
                .collect();
            IpcResponse::ok(serde_json::json!({
                "client_id": pull.client_id,
                "updates": updates,
                "current_generation": pull.delivery.current_generation,
                "last_ack_generation": pull.delivery.last_ack_generation,
                "pending_updates": pull.delivery.pending_updates,
            }))
        }
        Ok(None) => IpcResponse::err("crdt replica pull refused: document is not editor-attached"),
        Err(e) => IpcResponse::err(format!("crdt replica pull failed: {e}")),
    }
}

pub fn handle_replica_ack(
    file: &str,
    identity: &str,
    patch_id: &str,
    generation: u64,
) -> IpcResponse {
    match agent_doc_crdt_relay_io::ack_replica_update_for_file(
        Path::new(file),
        identity,
        patch_id,
        generation,
    ) {
        Ok(Some(acknowledged)) => IpcResponse::ok(serde_json::json!({
            "acknowledged": acknowledged,
        })),
        Ok(None) => IpcResponse::err("crdt replica ack refused: document is not editor-attached"),
        Err(e) => IpcResponse::err(format!("crdt replica ack failed: {e}")),
    }
}

pub fn handle_replica_awareness(file: &str, identity: &str, awareness_b64: &str) -> IpcResponse {
    let json = match BASE64_STANDARD.decode(awareness_b64) {
        Ok(bytes) => bytes,
        Err(e) => return IpcResponse::err(format!("crdt awareness: bad base64: {e}")),
    };
    let state: AwarenessState = match serde_json::from_slice(&json) {
        Ok(state) => state,
        Err(e) => return IpcResponse::err(format!("crdt awareness: bad json: {e}")),
    };
    match agent_doc_crdt_relay_io::set_replica_awareness_for_file(Path::new(file), identity, state)
    {
        Ok(Some(snapshot)) => {
            let presence: Vec<serde_json::Value> = snapshot
                .iter()
                .map(|(client_id, state)| {
                    serde_json::json!({
                        "client_id": client_id,
                        "awareness_b64": BASE64_STANDARD
                            .encode(serde_json::to_vec(state).unwrap_or_default()),
                    })
                })
                .collect();
            IpcResponse::ok(serde_json::json!({ "presence": presence }))
        }
        Ok(None) => IpcResponse::err("crdt awareness refused: document is not editor-attached"),
        Err(e) => IpcResponse::err(format!("crdt awareness failed: {e}")),
    }
}

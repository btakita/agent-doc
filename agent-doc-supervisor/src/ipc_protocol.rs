//! Supervisor IPC protocol vocabulary.
//!
//! This module owns the wire-level request and response shapes shared by
//! supervisor IPC clients and the orchestration socket transport. It does not
//! bind sockets, connect to listeners, or perform command effects.

use serde::{Deserialize, Serialize};

/// IPC command sent by a client to the supervisor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum IpcMethod {
    Restart {
        #[serde(default = "default_restart_mode")]
        mode: String,
    },
    Inject {
        bytes: String,
    },
    /// Gate-exempt operator control injection (e.g. `/clear`). Unlike
    /// [`IpcMethod::Inject`], this bypasses the managed-capability dispatch gate
    /// so an operator can stop/clear a session whose capability proof failed
    /// without `kill -9`. Delivery is otherwise identical to `Inject`.
    Clear {
        bytes: String,
    },
    State,
    Pid,
    Stop {
        #[serde(default)]
        graceful: bool,
    },
    /// "Stop Agent": kill the harness child while keeping the supervisor process
    /// alive at its restart-or-quit keepalive prompt, so the operator can then
    /// restart the agent manually (e.g. press Enter). This is DISTINCT from
    /// [`IpcMethod::Stop`] (which exits the whole supervisor) and from
    /// `admin kill-supervisor` (which kills the supervisor process). The
    /// supervisor never auto-restarts after a `StopAgent`, regardless of the
    /// harness `clean_exit_behavior`.
    StopAgent {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    // --- CRDT live multi-editor delta fan-out (`#crdtauth5`, plan phase 5) ----
    //
    // These variants are ADDITIVE: they extend the control-plane enum with an
    // editor-replica lifecycle/delta family without touching the existing
    // variants or their handlers. All of them are routed through the per-document
    // `crdt_relay_host` hub registry and are authority-gated - on a document with
    // no live editor (`CrdtAuthority::GitAuthoritative` / Detached) the handler
    // refuses them and allocates no hub, so the headless control-plane path is
    // byte-for-byte unchanged.
    //
    // The wire carries `file` (the canonical document path, the per-document hub
    // key), an `identity` string (a stable editor-process identity that mints a
    // deterministic yrs client-id), and yrs update / state-vector bytes
    // base64-encoded (NDJSON is text; raw yrs bytes are not UTF-8).
    /// Register an editor replica with the document's relay hub. The supervisor
    /// creates/attaches the editor replica in the per-document hub and replies
    /// with the minted `client_id` plus the canonical replica's encoded state
    /// (base64) so the editor's FFI node bootstraps converged on first contact.
    ReplicaRegister {
        /// Canonical document path (the per-document hub key).
        file: String,
        /// Stable editor-process identity (mints a deterministic client-id).
        identity: String,
    },
    /// Deregister an editor replica (editor/IDE closed the document). Drops the
    /// hub-side mirror and expires the ephemeral awareness entry.
    ReplicaDeregister {
        file: String,
        identity: String,
    },
    /// Broadcast a yrs update from one editor replica: the supervisor applies it
    /// to that replica's hub-side mirror, integrates the new op(s) into the
    /// canonical replica, and fans the missing delta out to every OTHER live
    /// replica. The reply carries the per-target fan-out updates (base64) so a
    /// caller relaying for peers can deliver them, plus the canonical text length
    /// for diagnostics.
    ReplicaUpdate {
        file: String,
        identity: String,
        /// Base64-encoded yrs update bytes produced by the editor's FFI node.
        update_b64: String,
    },
    /// Pull pending supervisor-to-editor updates for this replica. Updates are
    /// retained until the editor applies and ACKs them via `replica_ack`.
    ReplicaPull {
        file: String,
        identity: String,
    },
    /// ACK one pending update after the editor has applied it locally.
    ReplicaAck {
        file: String,
        identity: String,
        patch_id: String,
        generation: u64,
    },
    /// Push an ephemeral awareness/presence update (cursor/selection). NOT part
    /// of the document CRDT, never persisted, never committed. Replies with the
    /// current presence snapshot (base64 JSON) for the other live replicas.
    ReplicaAwareness {
        file: String,
        identity: String,
        /// Base64-encoded JSON `agent_doc_document_realtime::crdt_relay::AwarenessState`.
        awareness_b64: String,
    },
}

fn default_restart_mode() -> String {
    "continue".to_string()
}

/// Return true when an IPC method is a real prompt dispatch that must pass the
/// managed-capability proof gate before delivery.
pub const fn ipc_method_requires_capability_gate(method: &IpcMethod) -> bool {
    matches!(method, IpcMethod::Inject { .. })
}

/// Build the canonical raw-PTY submit bytes for a single-line harness input.
///
/// This is only for direct child-PTY fallback writes. Tmux-bound submissions
/// use the shared `send-keys <text> Enter` helper and must not route through
/// this raw carriage-return encoding.
pub fn submit_bytes(text: &str) -> String {
    let payload = agent_doc_tmux_commands::submitted_text_without_trailing_line_endings(text);
    format!("{payload}\r")
}

/// Response from the supervisor to a client command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn ok_empty() -> Self {
        Self {
            ok: true,
            data: None,
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_mode_defaults_to_continue_when_absent() {
        assert_eq!(
            serde_json::from_str::<IpcMethod>(r#"{"method":"restart"}"#).unwrap(),
            IpcMethod::Restart {
                mode: "continue".to_string(),
            }
        );
    }

    #[test]
    fn stop_agent_serde_roundtrips_and_is_distinct_from_stop() {
        let with_reason = IpcMethod::StopAgent {
            reason: Some("menu".to_string()),
        };
        let json = serde_json::to_string(&with_reason).unwrap();
        assert_eq!(json, r#"{"method":"stop_agent","reason":"menu"}"#);
        assert_eq!(
            serde_json::from_str::<IpcMethod>(&json).unwrap(),
            with_reason
        );

        let no_reason = IpcMethod::StopAgent { reason: None };
        let json = serde_json::to_string(&no_reason).unwrap();
        assert_eq!(json, r#"{"method":"stop_agent"}"#);
        assert_eq!(serde_json::from_str::<IpcMethod>(&json).unwrap(), no_reason);
        assert_eq!(
            serde_json::from_str::<IpcMethod>(r#"{"method":"stop_agent"}"#).unwrap(),
            IpcMethod::StopAgent { reason: None }
        );

        assert_ne!(
            IpcMethod::StopAgent { reason: None },
            IpcMethod::Stop { graceful: false }
        );
        let stop_json = serde_json::to_string(&IpcMethod::Stop { graceful: false }).unwrap();
        assert!(stop_json.contains(r#""method":"stop""#));
        assert!(!stop_json.contains("stop_agent"));
    }

    #[test]
    fn crdt_replica_variants_serde_roundtrip_and_are_additive() {
        let reg = IpcMethod::ReplicaRegister {
            file: "plan.md".into(),
            identity: "intellij:1234".into(),
        };
        let json = serde_json::to_string(&reg).unwrap();
        assert_eq!(
            json,
            r#"{"method":"replica_register","file":"plan.md","identity":"intellij:1234"}"#
        );
        assert_eq!(serde_json::from_str::<IpcMethod>(&json).unwrap(), reg);

        let upd = IpcMethod::ReplicaUpdate {
            file: "plan.md".into(),
            identity: "vscode:99".into(),
            update_b64: "AAEC".into(),
        };
        assert_eq!(
            serde_json::from_str::<IpcMethod>(&serde_json::to_string(&upd).unwrap()).unwrap(),
            upd
        );

        let pull = IpcMethod::ReplicaPull {
            file: "plan.md".into(),
            identity: "vscode:99".into(),
        };
        assert_eq!(
            serde_json::to_string(&pull).unwrap(),
            r#"{"method":"replica_pull","file":"plan.md","identity":"vscode:99"}"#
        );
        assert_eq!(
            serde_json::from_str::<IpcMethod>(&serde_json::to_string(&pull).unwrap()).unwrap(),
            pull
        );

        let ack = IpcMethod::ReplicaAck {
            file: "plan.md".into(),
            identity: "vscode:99".into(),
            patch_id: "crdt:1:2:3".into(),
            generation: 3,
        };
        assert_eq!(
            serde_json::to_string(&ack).unwrap(),
            r#"{"method":"replica_ack","file":"plan.md","identity":"vscode:99","patch_id":"crdt:1:2:3","generation":3}"#
        );
        assert_eq!(
            serde_json::from_str::<IpcMethod>(&serde_json::to_string(&ack).unwrap()).unwrap(),
            ack
        );

        assert_eq!(
            serde_json::to_string(&IpcMethod::State).unwrap(),
            r#"{"method":"state"}"#
        );
        assert_eq!(
            serde_json::to_string(&IpcMethod::Stop { graceful: false }).unwrap(),
            r#"{"method":"stop","graceful":false}"#
        );
        assert_eq!(
            serde_json::to_string(&IpcMethod::Inject { bytes: "x".into() }).unwrap(),
            r#"{"method":"inject","bytes":"x"}"#
        );
        assert_eq!(
            serde_json::from_str::<IpcMethod>(r#"{"method":"pid"}"#).unwrap(),
            IpcMethod::Pid
        );
    }

    #[test]
    fn ipc_response_json_shape_omits_absent_fields() {
        assert_eq!(
            serde_json::to_string(&IpcResponse::ok(serde_json::json!({ "pid": 42 }))).unwrap(),
            r#"{"ok":true,"data":{"pid":42}}"#
        );
        assert_eq!(
            serde_json::to_string(&IpcResponse::ok_empty()).unwrap(),
            r#"{"ok":true}"#
        );
        assert_eq!(
            serde_json::to_string(&IpcResponse::err("no active session")).unwrap(),
            r#"{"ok":false,"error":"no active session"}"#
        );
    }

    #[test]
    fn ipc_method_gate_classification_only_gates_inject() {
        assert!(ipc_method_requires_capability_gate(&IpcMethod::Inject {
            bytes: "x".to_string(),
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Clear {
            bytes: "/clear".to_string(),
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Stop {
            graceful: false,
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Restart {
            mode: "continue".to_string(),
        }));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::State));
        assert!(!ipc_method_requires_capability_gate(&IpcMethod::Pid));
        assert!(!ipc_method_requires_capability_gate(
            &IpcMethod::ReplicaRegister {
                file: "doc.md".to_string(),
                identity: "editor".to_string(),
            },
        ));
    }

    #[test]
    fn raw_pty_submit_bytes_use_single_carriage_return_enter() {
        assert_eq!(submit_bytes("/clear"), "/clear\r");
        assert_eq!(submit_bytes("/clear\n"), "/clear\r");
        assert_eq!(submit_bytes("/clear\r\n"), "/clear\r");
    }
}

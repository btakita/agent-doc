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
    /// Realtime operator input for the currently active harness turn.
    ///
    /// Unlike `Inject`, this never starts a new agent-doc turn. The supervisor
    /// accepts it only while its authoritative actor is busy and deduplicates
    /// retries by `steering_id`.
    Steer {
        steering_id: String,
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
}

fn default_restart_mode() -> String {
    "continue".to_string()
}

/// Return true when an IPC method is a real prompt dispatch that must pass the
/// managed-capability proof gate before delivery.
pub const fn ipc_method_requires_capability_gate(method: &IpcMethod) -> bool {
    matches!(method, IpcMethod::Inject { .. } | IpcMethod::Steer { .. })
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
    fn crdt_replica_variants_are_not_supervisor_ipc() {
        for json in [
            r#"{"method":"replica_register","file":"plan.md","identity":"intellij:1234"}"#,
            r#"{"method":"replica_deregister","file":"plan.md","identity":"intellij:1234"}"#,
            r#"{"method":"replica_update","file":"plan.md","identity":"vscode:99","update_b64":"AAEC"}"#,
            r#"{"method":"replica_pull","file":"plan.md","identity":"vscode:99"}"#,
            r#"{"method":"replica_ack","file":"plan.md","identity":"vscode:99","patch_id":"crdt:1:2:3","generation":3}"#,
            r#"{"method":"replica_awareness","file":"plan.md","identity":"vscode:99","awareness_b64":"e30="}"#,
            r#"{"method":"crdt_current_text","file":"plan.md","source":"resolve_current_doc"}"#,
            r#"{"method":"crdt_checkpoint","file":"plan.md","source":"admin_reload_lib"}"#,
        ] {
            assert!(
                serde_json::from_str::<IpcMethod>(json).is_err(),
                "supervisor IPC must not accept editor CRDT method: {json}"
            );
        }

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
            serde_json::to_string(&IpcMethod::Steer {
                steering_id: "steer-1".into(),
                bytes: "keep going".into(),
            })
            .unwrap(),
            r#"{"method":"steer","steering_id":"steer-1","bytes":"keep going"}"#
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
    }

    #[test]
    fn raw_pty_submit_bytes_use_single_carriage_return_enter() {
        assert_eq!(submit_bytes("/clear"), "/clear\r");
        assert_eq!(submit_bytes("/clear\n"), "/clear\r");
        assert_eq!(submit_bytes("/clear\r\n"), "/clear\r");
    }
}

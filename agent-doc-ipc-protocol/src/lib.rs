//! Pure IPC protocol classification policy for agent-doc.
//!
//! This crate owns reusable decisions about plugin IPC payloads. It does not
//! create sockets, read or write files, spawn listener threads, or mutate
//! documents.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackRequest {
    pub doc_path: String,
    pub doc_hash: String,
    pub operations: Vec<String>,
    pub context: Option<String>,
    pub created_at: u64,
    pub ttl_secs: u64,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackPatch {
    pub component: String,
    pub mode: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackResponse {
    pub request_id: String,
    pub status: String,
    pub summary: String,
    pub details: Option<String>,
    pub patches: Option<Vec<CallbackPatch>>,
    pub completed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingCallback {
    pub doc_path: String,
    pub operations: Vec<String>,
    pub urgency: String,
}

pub fn callback_request(
    doc_path: impl Into<String>,
    doc_hash: impl Into<String>,
    operations: impl IntoIterator<Item = impl Into<String>>,
    context: Option<impl Into<String>>,
    created_at: u64,
    ttl_secs: u64,
    request_id: impl Into<String>,
) -> CallbackRequest {
    CallbackRequest {
        doc_path: doc_path.into(),
        doc_hash: doc_hash.into(),
        operations: operations.into_iter().map(Into::into).collect(),
        context: context.map(Into::into),
        created_at,
        ttl_secs,
        request_id: request_id.into(),
    }
}

pub fn callback_response(
    request_id: impl Into<String>,
    status: impl Into<String>,
    summary: impl Into<String>,
    details: Option<impl Into<String>>,
    patches: Option<Vec<CallbackPatch>>,
    completed_at: u64,
) -> CallbackResponse {
    CallbackResponse {
        request_id: request_id.into(),
        status: status.into(),
        summary: summary.into(),
        details: details.map(Into::into),
        patches,
        completed_at,
    }
}

pub fn callback_request_is_expired(request: &CallbackRequest, now: u64) -> bool {
    now.saturating_sub(request.created_at) > request.ttl_secs
}

pub fn callback_urgency_for_elapsed(elapsed_secs: u64) -> &'static str {
    if elapsed_secs < 10 {
        "high"
    } else if elapsed_secs < 60 {
        "normal"
    } else {
        "low"
    }
}

pub fn pending_callback_from_request(request: CallbackRequest, now: u64) -> PendingCallback {
    PendingCallback {
        doc_path: request.doc_path,
        operations: request.operations,
        urgency: callback_urgency_for_elapsed(now.saturating_sub(request.created_at)).to_string(),
    }
}

pub fn callback_response_matches_request(
    response: &CallbackResponse,
    request: Option<&CallbackRequest>,
) -> bool {
    request
        .map(|request| response.request_id == request.request_id)
        .unwrap_or(true)
}

/// Classification of a plugin-sent IPC ack line.
///
/// The plugin sends a JSON ack after applying a patch. `Ok` means the patch was
/// applied normally. `AlreadyApplied` means the plugin detected the response
/// body is already present in the live buffer and chose not to re-apply it.
/// `Failed` covers any other `status: error` ack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckClassification {
    Ok,
    AlreadyApplied,
    Failed,
    /// Early `pending`/`accepted` ack: the listener received the patch but has
    /// not applied it yet. Liveness only; the sender must keep waiting for a
    /// terminal ack.
    Pending,
}

/// Classify a plugin-sent IPC ack line.
pub fn classify_ack(ack: &str) -> AckClassification {
    let Some(value) = serde_json::from_str::<serde_json::Value>(ack).ok() else {
        return AckClassification::Ok;
    };
    let status = value.get("status").and_then(|s| s.as_str());
    if let Some(s) = status
        && (s.eq_ignore_ascii_case("pending") || s.eq_ignore_ascii_case("accepted"))
    {
        return AckClassification::Pending;
    }
    let status_is_error = status
        .map(|s| s.eq_ignore_ascii_case("error"))
        .unwrap_or(false);
    if !status_is_error {
        return AckClassification::Ok;
    }
    let reason = value
        .get("reason")
        .and_then(|r| r.as_str())
        .map(|r| r.to_ascii_lowercase());
    match reason.as_deref() {
        Some("already_applied") => AckClassification::AlreadyApplied,
        _ => AckClassification::Failed,
    }
}

/// Tag a `patch` message with the `early_ack` opt-in when enabled.
///
/// Non-patch traffic is returned unchanged so queue convergence, VCS refreshes,
/// and read-only live-buffer proof requests never accidentally request a
/// two-phase patch ack.
pub fn early_ack_tagged_message(message: &serde_json::Value, enabled: bool) -> serde_json::Value {
    if !enabled {
        return message.clone();
    }
    let is_patch = message
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t == "patch")
        .unwrap_or(false);
    if !is_patch {
        return message.clone();
    }
    let mut tagged = message.clone();
    if let Some(obj) = tagged.as_object_mut() {
        obj.insert("early_ack".to_string(), serde_json::Value::Bool(true));
    }
    tagged
}

/// True when an incoming listener message opts into early-ack.
pub fn message_requests_early_ack(message: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(message)
        .ok()
        .and_then(|v| v.get("early_ack").and_then(|e| e.as_bool()))
        .unwrap_or(false)
}

/// The early `pending` ack line emitted by an early-ack-aware listener on patch
/// receipt before the patch is applied.
pub fn early_ack_line() -> &'static str {
    r#"{"type":"ack","status":"pending"}"#
}

/// Ops-log marker recorded when an early `pending` ack is emitted before the
/// blocking apply handler runs.
pub fn early_ack_ops_marker() -> &'static str {
    "[ipc-socket] early_ack_pending emitted before apply"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FullContentIpcMode {
    /// Late fallback repair for an agent response. Must not dirty an already
    /// committed cycle.
    ResponseFallback,
    /// Operator-owned replacement such as Compact Exchange. This is a new
    /// document mutation even when the previous response cycle is committed.
    OperatorMutation,
}

impl FullContentIpcMode {
    pub const fn source_label(self) -> &'static str {
        match self {
            Self::ResponseFallback => "response_fallback",
            Self::OperatorMutation => "compact_exchange",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AckClassification, FullContentIpcMode, callback_request, callback_request_is_expired,
        callback_response, callback_response_matches_request, callback_urgency_for_elapsed,
        classify_ack, early_ack_line, early_ack_ops_marker, early_ack_tagged_message,
        message_requests_early_ack, pending_callback_from_request,
    };

    #[test]
    fn classify_ack_treats_pending_status_as_pending() {
        assert_eq!(
            classify_ack(r#"{"type":"ack","status":"pending"}"#),
            AckClassification::Pending
        );
        assert_eq!(
            classify_ack(r#"{"type":"ack","status":"accepted"}"#),
            AckClassification::Pending
        );
        assert_eq!(classify_ack(early_ack_line()), AckClassification::Pending);
    }

    #[test]
    fn classify_ack_treats_ok_status_as_ok() {
        let ack = r#"{"type":"ack","status":"ok","id":"patch-123"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Ok);
    }

    #[test]
    fn classify_ack_treats_ack_without_status_as_ok() {
        let ack = r#"{"type":"ack","id":"patch-123"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Ok);
    }

    #[test]
    fn classify_ack_treats_already_applied_reason_as_already_applied() {
        let ack = r#"{"type":"ack","status":"error","reason":"already_applied"}"#;
        assert_eq!(classify_ack(ack), AckClassification::AlreadyApplied);
    }

    #[test]
    fn classify_ack_treats_already_applied_reason_uppercase_as_already_applied() {
        let ack = r#"{"type":"ack","status":"ERROR","reason":"Already_Applied"}"#;
        assert_eq!(classify_ack(ack), AckClassification::AlreadyApplied);
    }

    #[test]
    fn classify_ack_treats_other_error_reasons_as_failed() {
        let ack = r#"{"type":"ack","status":"error","reason":"apply_failed"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Failed);
    }

    #[test]
    fn classify_ack_treats_error_status_without_reason_as_failed() {
        let ack = r#"{"type":"ack","status":"error"}"#;
        assert_eq!(classify_ack(ack), AckClassification::Failed);
    }

    #[test]
    fn classify_ack_treats_malformed_json_as_ok() {
        let ack = "not json at all";
        assert_eq!(classify_ack(ack), AckClassification::Ok);
    }

    #[test]
    fn message_requests_early_ack_reads_flag() {
        assert!(message_requests_early_ack(
            r#"{"type":"patch","early_ack":true}"#
        ));
        assert!(!message_requests_early_ack(r#"{"type":"patch"}"#));
        assert!(!message_requests_early_ack(
            r#"{"type":"patch","early_ack":false}"#
        ));
        assert!(!message_requests_early_ack("not json"));
    }

    #[test]
    fn early_ack_tagging_marks_only_enabled_patch_messages() {
        let patch = serde_json::json!({"type": "patch", "file": "x.md"});
        let tagged = early_ack_tagged_message(&patch, true);
        assert_eq!(tagged["early_ack"], serde_json::Value::Bool(true));
        assert_eq!(tagged["type"], "patch");
        assert_eq!(tagged["file"], "x.md");
        assert!(message_requests_early_ack(&tagged.to_string()));

        assert_eq!(early_ack_tagged_message(&patch, false), patch);

        let other = serde_json::json!({"type": "vcs_refresh"});
        assert_eq!(early_ack_tagged_message(&other, true), other);
    }

    #[test]
    fn early_ack_ops_marker_carries_predicate_token() {
        let marker = early_ack_ops_marker();
        assert!(
            marker.contains("early_ack_pending"),
            "ops marker must carry the predicate token: {marker}"
        );
        assert!(!early_ack_line().contains("early_ack_pending"));
    }

    #[test]
    fn callback_request_expiry_uses_request_ttl() {
        let request = callback_request(
            "/tmp/plan.md",
            "doc-hash",
            ["compact"],
            None::<String>,
            100,
            60,
            "request-1",
        );

        assert!(!callback_request_is_expired(&request, 160));
        assert!(callback_request_is_expired(&request, 161));
    }

    #[test]
    fn callback_pending_urgency_is_elapsed_time_bucket() {
        assert_eq!(callback_urgency_for_elapsed(0), "high");
        assert_eq!(callback_urgency_for_elapsed(9), "high");
        assert_eq!(callback_urgency_for_elapsed(10), "normal");
        assert_eq!(callback_urgency_for_elapsed(59), "normal");
        assert_eq!(callback_urgency_for_elapsed(60), "low");
    }

    #[test]
    fn callback_pending_projection_keeps_doc_and_operations() {
        let request = callback_request(
            "/tmp/plan.md",
            "doc-hash",
            ["compact", "summary"],
            Some("operator context"),
            100,
            300,
            "request-1",
        );

        let pending = pending_callback_from_request(request, 115);

        assert_eq!(pending.doc_path, "/tmp/plan.md");
        assert_eq!(pending.operations, vec!["compact", "summary"]);
        assert_eq!(pending.urgency, "normal");
    }

    #[test]
    fn callback_response_match_rejects_stale_request_id() {
        let request = callback_request(
            "/tmp/plan.md",
            "doc-hash",
            ["compact"],
            None::<String>,
            100,
            300,
            "request-1",
        );
        let matching = callback_response("request-1", "success", "done", None::<String>, None, 150);
        let stale = callback_response("request-2", "success", "done", None::<String>, None, 150);

        assert!(callback_response_matches_request(&matching, Some(&request)));
        assert!(!callback_response_matches_request(&stale, Some(&request)));
        assert!(callback_response_matches_request(&stale, None));
    }

    #[test]
    fn full_content_ipc_mode_source_labels_are_stable() {
        assert_eq!(
            FullContentIpcMode::ResponseFallback.source_label(),
            "response_fallback"
        );
        assert_eq!(
            FullContentIpcMode::OperatorMutation.source_label(),
            "compact_exchange"
        );
    }
}

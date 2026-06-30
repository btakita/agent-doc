//! Pure IPC protocol classification policy for agent-doc.
//!
//! This crate owns reusable decisions about plugin IPC payloads. It does not
//! create sockets, read or write files, spawn listener threads, or mutate
//! documents.

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

#[cfg(test)]
mod tests {
    use super::{
        AckClassification, classify_ack, early_ack_line, early_ack_ops_marker,
        early_ack_tagged_message, message_requests_early_ack,
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
}

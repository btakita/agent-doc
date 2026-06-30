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

#[cfg(test)]
mod tests {
    use super::{AckClassification, classify_ack};

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
}

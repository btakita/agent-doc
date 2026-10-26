//! The editor-replica attach-failure diagnosis behind the hot-reloaded native
//! boundary (GH #326 phase 5b, the #295 "path 2" lever).
//!
//! When a JetBrains editor cannot attach its CRDT replica, the plugin records a
//! failure *reason* (`controller_error: ...`, `ambiguous-retained-projection`,
//! `native-ffi-unavailable`, ...). What that reason means for the operator was
//! decided in Kotlin: the one-line remedy, the `attach_refused_<token>` ops-log
//! status, whether the hold must be surfaced without waiting for an editor
//! action, whether the refusal needs a reliable-liveness republish, and the
//! notification text. Every new failure reason or remedy wording needed a plugin
//! generation, and on the exact-263 modular package that is an IDE restart.
//!
//! Those rules live here now, as the `attach_failure_diagnosis` derivation. The
//! plugin reports the recorded reason (and, for the operator notification, the
//! document's file name) and executes the answer: it writes the ops-log event,
//! shows the notification, republishes liveness, and schedules the retry, in
//! the same order as before. Every caller asks off the EDT.
//!
//! The plugin keeps a frozen Kotlin copy of the pre-#326 rules for a library
//! that cannot answer. One parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/attach_failure_diagnosis.json`)
//! is answered by both. New rules land here, never there.

use serde::{Deserialize, Serialize};

/// What one attach failure reports. Every field is optional on the wire: after
/// a hot reload this library is newer than the plugin generation calling it. An
/// absent reason is the empty reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachFailureEvidence {
    /// The recorded failure reason, exactly as the registration path recorded it.
    #[serde(default)]
    pub reason: Option<String>,
    /// The document's file name, for the operator notification. Without it the
    /// answer carries no `hold_message`.
    #[serde(default)]
    pub file_name: Option<String>,
}

/// The `attach_failure_diagnosis` answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachFailureDiagnosis {
    /// A one-line operator remedy, or `None` when there is no advice beyond the
    /// reason itself.
    pub remedy: Option<String>,
    /// The single ops-log token: the plugin records `attach_refused_<token>`.
    pub status_token: String,
    /// The failure is a fail-closed projection hold that must be persisted and
    /// surfaced to the operator without waiting for an editor action.
    pub needs_operator_surface: bool,
    /// The owning controller names another generation's endpoint, so the
    /// plugin republishes reliable liveness and retries promptly.
    pub needs_liveness_republish: bool,
    /// Whether the adapter may schedule another attach attempt without new
    /// external evidence. Projection holds require the operator to close,
    /// repair, and reopen the document, so polling them cannot change the
    /// answer and only creates a background register/deregister loop.
    pub retry_disposition: AttachFailureRetryDisposition,
    /// The operator notification for a hold, present exactly when the evidence
    /// carries a file name.
    pub hold_message: Option<String>,
}

/// The next admissible registration trigger after an attach failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachFailureRetryDisposition {
    /// A transient failure may use the ordinary capped backoff timer.
    Automatic,
    /// No timer may run; a close/repair/reopen or another explicit recovery
    /// event must supply new evidence before registration is attempted again.
    AwaitOperatorEvidence,
}

/// Diagnose one attach failure reason.
pub fn diagnose_attach_failure(evidence: &AttachFailureEvidence) -> AttachFailureDiagnosis {
    let reason = evidence.reason.as_deref().unwrap_or("");
    let remedy = attach_failure_remedy(reason);
    let needs_operator_surface = needs_operator_surface(reason);
    let hold_message = evidence
        .file_name
        .as_deref()
        .map(|file_name| hold_message(file_name, reason, remedy));
    AttachFailureDiagnosis {
        remedy: remedy.map(str::to_string),
        status_token: status_token(reason),
        needs_operator_surface,
        needs_liveness_republish: reason.contains("replica_register_stale_editor_endpoint"),
        retry_disposition: if needs_operator_surface {
            AttachFailureRetryDisposition::AwaitOperatorEvidence
        } else {
            AttachFailureRetryDisposition::Automatic
        },
        hold_message,
    }
}

/// The operator remedy for [reason]; the first matching rule wins.
///
/// `detached_authority` on a *registration* means the controller saw no live
/// editor PID for the request. Registration is what establishes editor
/// authority, so the controller does not refuse it for lacking authority: it
/// refuses when the request carries no live PID, which is what a plugin
/// generation loaded before that field existed sends. The IDE keeps the
/// generation it started with, so only an IDE restart replaces it.
fn attach_failure_remedy(reason: &str) -> Option<&'static str> {
    if reason.contains("detached_authority") {
        Some(
            "Restart the IDE. The controller saw no live editor PID on the registration, which is \
             what a plugin generation loaded before the current install sends. Reloading the \
             native library or recycling the controller cannot replace a loaded plugin generation.",
        )
    } else if reason.contains("socket_unavailable") {
        Some(
            "No project controller is listening for that document's project root; start or recycle it.",
        )
    } else if reason.contains("not_agent_doc_document") {
        Some(
            "That file is not an agent-doc session document (no agent_doc_* frontmatter, no <!-- agent: markers).",
        )
    } else if reason.contains("retained-reseed-missing-settled-shadow") {
        Some(
            "The restarted editor has no settled ancestor from which it can safely seed the retained \
             controller projection. Copy any unsaved text, close this editor tab, run `agent-doc repair \
             <file>` while it is detached, and reopen it; restarting the editor again cannot create that ancestor.",
        )
    } else if reason.contains("ambiguous-retained-projection")
        || reason.contains("ambiguous-reregister-projection")
    {
        // `#lagcanonicalrollforward`: after the dual proof a hold means no
        // direction is proven lossless, not that the buffer diverged: a buffer
        // can equal its settled shadow while canonical's descent from it is
        // unproven. The buffer is never touched while held, so the operator
        // keeps whatever they copy.
        Some(
            "Agent Doc could not prove that adopting the controller canonical, publishing this buffer, or \
             merging the two keeps every edit, so the buffer was not changed. Copy any editor text you need \
             to keep, close this editor tab, run `agent-doc repair <file>` while it is detached, reopen it, \
             and paste back anything missing.",
        )
    } else if reason.contains("attach-timeout-pending") {
        Some(
            "Registration is still running; wait for the next editor status update before retrying.",
        )
    } else if reason.contains("native-ffi-unavailable") {
        Some(
            "Inspect About Agent Doc or the IDE log for the executable and library that failed to load. \
             Fix or remove that incompatible install so PATH resolves to the intended agent-doc; the native \
             loader retries automatically. `agent-doc admin reload-lib` cannot reach this editor until FFI loads.",
        )
    } else if reason.contains("native-handoff-timeout") {
        Some(
            "The native-generation handoff did not finish within the editor action budget. Wait for reload to \
             settle, then run `agent-doc admin reload-lib` once before retrying.",
        )
    } else if reason.contains("attach-exception") || reason.contains("attach-worker-failed") {
        Some(
            "Inspect the IDE log and the `editor_surface_event` attach-refused entry in `.agent-doc/logs/ops.log`, \
             then repair the named cause before retrying.",
        )
    } else if reason.contains("unknown-attach-refusal") || reason.contains("controller-register") {
        Some(
            "Run `agent-doc admin inspect <file> --json` and compare editor_replica.live_editors with live_replicas.",
        )
    } else {
        None
    }
}

/// A fail-closed projection hold the operator must see without an editor action.
fn needs_operator_surface(reason: &str) -> bool {
    reason.contains("retained-reseed-missing-settled-shadow")
        || reason.contains("ambiguous-retained-projection")
        || reason.contains("ambiguous-reregister-projection")
}

/// The ops-log token: the reason before its first `:`, lowercased, with every
/// run of characters outside `[a-z0-9_-]` replaced by one `_`, and `_` trimmed
/// from both ends; `unknown` when nothing remains.
fn status_token(reason: &str) -> String {
    let head = reason.split(':').next().unwrap_or("").to_lowercase();
    let mut token = String::with_capacity(head.len());
    let mut in_run = false;
    for ch in head.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-' {
            token.push(ch);
            in_run = false;
        } else if !in_run {
            token.push('_');
            in_run = true;
        }
    }
    let token = token.trim_matches('_');
    if token.is_empty() {
        "unknown".to_string()
    } else {
        token.to_string()
    }
}

fn hold_message(file_name: &str, reason: &str, remedy: Option<&str>) -> String {
    let mut message = format!(
        "Agent Doc could not attach the open editor replica for {file_name}. Cause: {reason}."
    );
    if let Some(remedy) = remedy {
        message.push(' ');
        message.push_str(remedy);
    }
    message.push_str(" The editor buffer and disk were left unchanged.");
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnose(reason: &str) -> AttachFailureDiagnosis {
        diagnose_attach_failure(&AttachFailureEvidence {
            reason: Some(reason.to_string()),
            file_name: None,
        })
    }

    #[test]
    fn an_empty_observation_is_an_unknown_reason_without_advice() {
        let answer = diagnose_attach_failure(&AttachFailureEvidence::default());
        assert_eq!(answer.remedy, None);
        assert_eq!(answer.status_token, "unknown");
        assert!(!answer.needs_operator_surface);
        assert!(!answer.needs_liveness_republish);
        assert_eq!(
            answer.retry_disposition,
            AttachFailureRetryDisposition::Automatic
        );
        assert_eq!(answer.hold_message, None);
    }

    #[test]
    fn the_status_token_is_one_ops_log_word() {
        assert_eq!(
            diagnose("retained-reseed-missing-settled-shadow: details").status_token,
            "retained-reseed-missing-settled-shadow"
        );
        assert_eq!(
            diagnose("Attach worker failed").status_token,
            "attach_worker_failed"
        );
        assert_eq!(diagnose("__lead  trail__").status_token, "lead_trail");
        assert_eq!(diagnose(":::").status_token, "unknown");
    }

    #[test]
    fn the_first_matching_remedy_wins() {
        let answer = diagnose("detached_authority socket_unavailable");
        assert!(answer.remedy.unwrap().starts_with("Restart the IDE."));
    }

    #[test]
    fn a_hold_message_names_the_file_cause_and_remedy() {
        let answer = diagnose_attach_failure(&AttachFailureEvidence {
            reason: Some("ambiguous-reregister-projection".to_string()),
            file_name: Some("plan.md".to_string()),
        });
        assert!(answer.needs_operator_surface);
        assert_eq!(
            answer.retry_disposition,
            AttachFailureRetryDisposition::AwaitOperatorEvidence
        );
        let message = answer.hold_message.unwrap();
        assert!(message.starts_with(
            "Agent Doc could not attach the open editor replica for plan.md. \
             Cause: ambiguous-reregister-projection. Agent Doc could not prove"
        ));
        // `#lagcanonicalrollforward`: the remedy never claims the buffer
        // diverged; a held buffer can equal its settled shadow.
        assert!(!message.contains("diverged from their last settled ancestor"));
        assert!(message.contains("paste back anything missing"));
        assert!(message.ends_with(" The editor buffer and disk were left unchanged."));
    }

    #[test]
    fn the_answer_round_trips_through_snake_case_json() {
        let json = serde_json::to_value(diagnose(
            "controller_error: replica_register_stale_editor_endpoint",
        ))
        .unwrap();
        assert_eq!(json["status_token"], "controller_error");
        assert_eq!(json["needs_liveness_republish"], true);
        assert_eq!(json["retry_disposition"], "automatic");
        assert!(json["remedy"].is_null());
        assert!(json["hold_message"].is_null());
    }
}

//! Harness-native subagent-end payload (`#workerquotaautodetect`).
//!
//! Claude Code and Grok Build end a subagent with a hook payload. Grok sends
//! each top-level key in camelCase and snake_case at once, so this parser
//! reads [`serde_json::Value`] and looks up both spellings. A serde alias
//! would reject that object as a duplicate field.
//!
//! The worker id is the spawn id `queue start --worker` stored. `subagentType`
//! and `agent_type` name a kind of agent, not that id.

use serde_json::Value;

use crate::usage_limit::{
    HarnessUsageLimit, SHORT_HANDBACK_LINES, handback_lines, worker_handback_usage_limit,
};

/// Last lines of a transcript consulted when the hook's final message is
/// missing or still a stub. A long real report is never overridden by an
/// older unflushed tail.
const TRANSCRIPT_TAIL_LINES: usize = 40;

const STRUCTURED_QUOTA_TOKENS: &[&str] = &[
    "rate_limit",
    "usage_limit",
    "usage_limit_reached",
    "quota",
    "quota_exceeded",
    "credit_limit",
];

/// A subagent-end payload that names the worker to look up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentEnd {
    pub worker_id: String,
    pub event: String,
    pub cwd: Option<String>,
    pub workspace_root: Option<String>,
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub last_message: Option<String>,
    pub transcript_path: Option<String>,
    /// Worktree path only when the payload names one. The hook cwd is not one.
    pub worktree: Option<String>,
    pub branch: Option<String>,
    /// Structured `error` / `reason` that is exactly a known quota token.
    pub structured_reason: Option<String>,
}

/// Why [`parse_subagent_end`] did not return a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentEndParse {
    NotASubagentEnd,
    /// The event is a subagent end, and it carries no worker id.
    MissingWorkerId,
}

/// Read a subagent-end signal from a hook object.
///
/// A parent `Stop` with no worker id is [`SubagentEndParse::NotASubagentEnd`]:
/// the owner's turn end is not a worker quota event. `Stop` / `StopFailure`
/// become a signal only when a worker id is present (Grok remaps a subagent's
/// Stop hook onto that event). An object with a worker id and no event name is
/// a signal too, so the dedicated subagent-end command still works when the
/// harness omits the event.
pub fn parse_subagent_end(value: &Value) -> Result<SubagentEnd, SubagentEndParse> {
    if !value.is_object() {
        return Err(SubagentEndParse::NotASubagentEnd);
    }
    let event = string_field(value, &["hook_event_name", "hookEventName"]);
    let worker_id = worker_id(value);
    let subagent_event = event.as_deref().is_some_and(event_is_subagent_end);
    let stop_event = event.as_deref().is_some_and(event_is_stop);
    if subagent_event && worker_id.is_none() {
        return Err(SubagentEndParse::MissingWorkerId);
    }
    let Some(worker_id) = worker_id else {
        return Err(SubagentEndParse::NotASubagentEnd);
    };
    if !(subagent_event || stop_event || event.is_none()) {
        return Err(SubagentEndParse::NotASubagentEnd);
    }
    Ok(SubagentEnd {
        worker_id,
        event: event.unwrap_or_default(),
        cwd: string_field(value, &["cwd"]),
        workspace_root: string_field(value, &["workspace_root", "workspaceRoot"]),
        session_id: string_field(value, &["session_id", "sessionId"]),
        parent_session_id: string_field(
            value,
            &["parent_session_id", "parentSessionId", "parentSessionID"],
        ),
        last_message: string_field(
            value,
            &[
                "last_assistant_message",
                "lastAssistantMessage",
                "last_message",
            ],
        ),
        transcript_path: string_field(
            value,
            &[
                "agent_transcript_path",
                "agentTranscriptPath",
                "transcript_path",
                "transcriptPath",
            ],
        ),
        worktree: string_field(value, &["worktree_path", "worktreePath", "worktree"]),
        branch: string_field(value, &["branch", "git_branch", "gitBranch"]),
        structured_reason: structured_quota_token(value),
    })
}

/// The usage limit this subagent end proves, if any.
///
/// The final message wins. A message longer than a short hand-back that does
/// not itself classify is a real report: an older transcript tail cannot
/// override it. A structured quota token still counts, including on that
/// report, because the harness named the stop reason exactly. Unknown prose
/// is not guessed at.
pub fn usage_limit_from_end(
    last_message: Option<&str>,
    transcript_tail: Option<&str>,
    structured_reason: Option<&str>,
) -> Option<HarnessUsageLimit> {
    let message = last_message.map(str::trim).filter(|text| !text.is_empty());
    if let Some(message) = message {
        if let Some(limit) = worker_handback_usage_limit(message) {
            return Some(limit);
        }
        if handback_lines(message).len() > SHORT_HANDBACK_LINES {
            return structured_reason.map(HarnessUsageLimit::reported);
        }
    }
    if let Some(tail) = transcript_tail
        .map(str::trim)
        .filter(|text| !text.is_empty())
        && let Some(limit) = transcript_tail_usage_limit(tail)
    {
        return Some(limit);
    }
    structured_reason.map(HarnessUsageLimit::reported)
}

fn transcript_tail_usage_limit(tail: &str) -> Option<HarnessUsageLimit> {
    let lines = tail
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let start = lines.len().saturating_sub(TRANSCRIPT_TAIL_LINES);
    let window = &lines[start..];
    let end = window.len().saturating_sub(SHORT_HANDBACK_LINES);
    worker_handback_usage_limit(&window[end..].join("\n"))
}

fn worker_id(value: &Value) -> Option<String> {
    string_field(
        value,
        &[
            "subagent_id",
            "subagentId",
            "agent_id",
            "agentId",
            "worker_id",
            "workerId",
        ],
    )
}

fn event_is_subagent_end(event: &str) -> bool {
    matches!(
        normalize_event(event).as_str(),
        "subagentstop" | "subagentend" | "subagentfinished"
    )
}

fn event_is_stop(event: &str) -> bool {
    matches!(normalize_event(event).as_str(), "stop" | "stopfailure")
}

fn normalize_event(event: &str) -> String {
    event
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match value.get(*key)? {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        _ => None,
    })
}

fn structured_quota_token(value: &Value) -> Option<String> {
    for key in [
        "error",
        "error_type",
        "errorType",
        "stop_reason",
        "stopReason",
        "reason",
    ] {
        if let Some(token) = quota_token(value.get(key)) {
            return Some(token);
        }
    }
    None
}

fn quota_token(value: Option<&Value>) -> Option<String> {
    let value = value?;
    match value {
        Value::String(text) => known_quota_token(text),
        Value::Object(map) => ["type", "code", "reason", "error"].iter().find_map(|key| {
            map.get(*key)
                .and_then(Value::as_str)
                .and_then(known_quota_token)
        }),
        _ => None,
    }
}

/// Exact token only. A reason that merely contains "limit" is not a quota stop.
fn known_quota_token(raw: &str) -> Option<String> {
    let token = raw.trim().to_ascii_lowercase();
    if token.is_empty() || token.contains(char::is_whitespace) {
        return None;
    }
    let token = token.replace('-', "_");
    STRUCTURED_QUOTA_TOKENS
        .iter()
        .any(|known| *known == token)
        .then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: &str = "You've hit your session limit · resets 9pm";

    fn both_spellings(worker: &str, message: &str, event: &str) -> Value {
        serde_json::json!({
            "hookEventName": "subagent_stop",
            "hook_event_name": event,
            "sessionId": "parent",
            "session_id": "parent",
            "subagentId": worker,
            "subagent_id": worker,
            "subagentType": "general",
            "lastAssistantMessage": message,
            "last_assistant_message": message,
            "cwd": "/work/tree",
            "workspaceRoot": "/work/main",
            "workspace_root": "/work/main",
        })
    }

    #[test]
    fn both_json_spellings_name_the_worker_not_the_type() {
        let signal = parse_subagent_end(&both_spellings("worker-sdk", LIMIT, "SubagentStop"))
            .expect("signal");
        assert_eq!(signal.worker_id, "worker-sdk");
        assert_eq!(signal.workspace_root.as_deref(), Some("/work/main"));
        assert!(signal.worktree.is_none());
        let limit =
            usage_limit_from_end(signal.last_message.as_deref(), None, None).expect("limit");
        assert_eq!(limit.limit, "session limit");
        assert_eq!(limit.harness.as_deref(), Some("claude"));
    }

    #[test]
    fn a_parent_stop_without_a_worker_id_is_not_a_subagent_end() {
        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "last_assistant_message": LIMIT,
            "subagentType": "general",
        });
        assert_eq!(
            parse_subagent_end(&payload),
            Err(SubagentEndParse::NotASubagentEnd)
        );
    }

    #[test]
    fn a_subagent_event_without_a_worker_id_is_named_missing() {
        let payload = serde_json::json!({
            "hook_event_name": "SubagentStop",
            "subagentType": "general",
            "last_assistant_message": LIMIT,
        });
        assert_eq!(
            parse_subagent_end(&payload),
            Err(SubagentEndParse::MissingWorkerId)
        );
    }

    #[test]
    fn stop_with_a_worker_id_is_a_signal_and_agent_type_is_not_the_id() {
        let payload = serde_json::json!({
            "hook_event_name": "Stop",
            "agent_type": "general",
            "agent_id": "worker-sdk",
            "reason": "end_turn",
            "last_assistant_message": "Done.",
        });
        let signal = parse_subagent_end(&payload).expect("signal");
        assert_eq!(signal.worker_id, "worker-sdk");
        assert!(signal.structured_reason.is_none());
    }

    #[test]
    fn a_real_report_quoting_a_limit_is_not_a_quota_stop() {
        let report = include_str!("fixtures/usage_limit/worker_real_report_quoting_limit.txt");
        assert!(usage_limit_from_end(Some(report), None, None).is_none());
    }

    #[test]
    fn a_substantial_report_is_not_overridden_by_an_older_transcript_tail() {
        let report = "\
One finished step.\n\
Two.\n\
Three.\n\
Four.\n\
Five. The phrase `You've hit your session limit` here is a quotation.\n";
        assert!(usage_limit_from_end(Some(report), Some(LIMIT), None).is_none());
    }

    #[test]
    fn a_short_stub_falls_through_to_the_transcript_tail() {
        let tail = format!("earlier line\n{LIMIT}\n");
        let limit = usage_limit_from_end(Some("Still working."), Some(&tail), None).expect("limit");
        assert_eq!(limit.limit, "session limit");
    }

    #[test]
    fn a_structured_rate_limit_counts_even_on_a_long_report() {
        let report = include_str!("fixtures/usage_limit/worker_real_report_quoting_limit.txt");
        let payload = serde_json::json!({
            "hook_event_name": "StopFailure",
            "subagentId": "worker-sdk",
            "error": "rate_limit",
            "last_assistant_message": report,
        });
        let signal = parse_subagent_end(&payload).expect("signal");
        assert_eq!(signal.structured_reason.as_deref(), Some("rate_limit"));
        let limit = usage_limit_from_end(
            signal.last_message.as_deref(),
            None,
            signal.structured_reason.as_deref(),
        )
        .expect("structured limit");
        assert_eq!(limit.limit, "usage limit");
        assert!(limit.evidence.contains("rate_limit"));
    }

    #[test]
    fn unrelated_stop_reasons_are_not_quota_tokens() {
        for reason in [
            "end_turn",
            "channel_closed",
            "interrupted",
            "max_output_tokens",
        ] {
            let payload = serde_json::json!({
                "hook_event_name": "Stop",
                "agent_id": "worker-sdk",
                "reason": reason,
                "last_assistant_message": "Done.",
            });
            let signal = parse_subagent_end(&payload).expect("signal");
            assert!(signal.structured_reason.is_none(), "{reason}");
            assert!(usage_limit_from_end(Some("Done."), None, None).is_none());
        }
    }
}

use agent_doc_turn::CyclePhase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardResult {
    None,
    Warn(Vec<String>),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockedCloseoutMessage<'a> {
    pub file: &'a str,
    pub kind: &'a str,
    pub cycle_id: &'a str,
    pub phase: CyclePhase,
    pub last_event: &'a str,
    pub source: &'a str,
    pub reason: &'a str,
    pub patch_id: Option<&'a str>,
    pub recovery: Option<&'a str>,
    pub detail: Option<&'a str>,
    pub recovery_command: Option<&'a str>,
    pub editor_authority_note: &'a str,
}

pub fn blocked_closeout_message(input: BlockedCloseoutMessage<'_>) -> String {
    let patch = input
        .patch_id
        .map(|id| format!(" patch_id={id}"))
        .unwrap_or_default();
    let recovery = input
        .recovery
        .map(|value| format!(" recovery={value}"))
        .unwrap_or_default();
    let detail = input
        .detail
        .map(|value| format!(" detail={value}"))
        .unwrap_or_default();
    let retry = input
        .recovery_command
        .map(str::to_string)
        .unwrap_or_else(|| format!("agent-doc write --commit {}", input.file));
    format!(
        "[session-check] INTERRUPTED: closeout blocked by `{}` for cycle `{}` (phase={} last_event={} source={} reason={}{}{}{}).{} The response/patch is retained for editor retry; save or resolve the live editor buffer, then run `{}`. Use `{} --force-disk` only after an explicit operator decision to override the live-editor safety guard.",
        input.kind,
        input.cycle_id,
        input.phase.as_str(),
        input.last_event,
        input.source,
        input.reason,
        patch,
        recovery,
        detail,
        input.editor_authority_note,
        retry,
        retry,
    )
}

pub fn open_cycle_detail(phase: CyclePhase) -> &'static str {
    match phase {
        CyclePhase::PreflightStarted => "cycle started but no write/commit followed",
        CyclePhase::ResponseCaptured => "response was captured but no write/commit followed",
        CyclePhase::WriteApplied => "response write landed but no terminal commit followed",
        CyclePhase::Committed => "no terminal commit followed",
        CyclePhase::Abandoned => "cycle was abandoned",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCycleMessage<'a> {
    pub file: &'a str,
    pub cycle_id: &'a str,
    pub phase: CyclePhase,
    pub last_event: &'a str,
    pub ipc_hint: &'a str,
}

pub fn open_cycle_message(input: OpenCycleMessage<'_>) -> String {
    if input.last_event.starts_with("direct_invocation_timeout")
        || input
            .last_event
            .starts_with("recursive_direct_invocation_blocked")
    {
        return format!(
            "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — direct invocation did not reach response capture. If the owning pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status {}` (or `agent-doc session clear {}`). Otherwise retry from outside the managed pane, restart the owner with `agent-doc start {}`, or abandon the stale cycle only after confirming no response exists.{}",
            input.cycle_id,
            input.phase.as_str(),
            input.last_event,
            input.file,
            input.file,
            input.file,
            input.ipc_hint
        );
    }
    format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — {}.{}",
        input.cycle_id,
        input.phase.as_str(),
        input.last_event,
        open_cycle_detail(input.phase),
        input.ipc_hint
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenCycleManualPatchbackMessage<'a> {
    pub file: &'a str,
    pub cycle_id: &'a str,
    pub phase: CyclePhase,
    pub last_event: &'a str,
    pub marker: &'a str,
}

pub fn open_cycle_manual_patchback_message(input: OpenCycleManualPatchbackMessage<'_>) -> String {
    format!(
        "[session-check] INTERRUPTED: cycle `{}` is still `{}` ({}) — found visible response patchback {} that is still outside the commit boundary. This looks like a manual repair that stopped before commit; finish it with `agent-doc write --commit {}` if you still have the response body, or commit the repaired document manually once the response is correct.",
        input.cycle_id,
        input.phase.as_str(),
        input.last_event,
        input.marker,
        input.file
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_closeout_message_uses_default_retry_and_optional_fields() {
        let message = blocked_closeout_message(BlockedCloseoutMessage {
            file: "task.md",
            kind: "editor_convergence_required",
            cycle_id: "cycle-1",
            phase: CyclePhase::WriteApplied,
            last_event: "write_applied",
            source: "write",
            reason: "live buffer differs",
            patch_id: Some("patch-7"),
            recovery: Some("retry"),
            detail: Some("needs save"),
            recovery_command: None,
            editor_authority_note: " Live editor `jb` lacks capability.",
        });

        assert!(message.contains("closeout blocked by `editor_convergence_required`"));
        assert!(message.contains("phase=write_applied"));
        assert!(message.contains("patch_id=patch-7 recovery=retry detail=needs save"));
        assert!(message.contains("Live editor `jb` lacks capability."));
        assert!(message.contains("run `agent-doc write --commit task.md`"));
        assert!(message.contains("Use `agent-doc write --commit task.md --force-disk`"));
    }

    #[test]
    fn blocked_closeout_message_honors_recovery_command() {
        let message = blocked_closeout_message(BlockedCloseoutMessage {
            file: "task.md",
            kind: "merge_blocked",
            cycle_id: "cycle-2",
            phase: CyclePhase::ResponseCaptured,
            last_event: "capture",
            source: "merge",
            reason: "conflict",
            patch_id: None,
            recovery: None,
            detail: None,
            recovery_command: Some("agent-doc write --commit task.md --retry-patch patch-9"),
            editor_authority_note: "",
        });

        assert!(message.contains("run `agent-doc write --commit task.md --retry-patch patch-9`"));
        assert!(
            message.contains(
                "Use `agent-doc write --commit task.md --retry-patch patch-9 --force-disk`"
            )
        );
        assert!(!message.contains("patch_id="));
    }

    #[test]
    fn open_cycle_message_formats_direct_invocation_recovery() {
        let message = open_cycle_message(OpenCycleMessage {
            file: "doc.md",
            cycle_id: "cycle-3",
            phase: CyclePhase::PreflightStarted,
            last_event: "direct_invocation_timeout after 30s",
            ipc_hint: " ipc proof pending",
        });

        assert!(message.contains("direct invocation did not reach response capture"));
        assert!(message.contains("agent-doc session status doc.md"));
        assert!(message.contains("agent-doc start doc.md"));
        assert!(message.ends_with(" ipc proof pending"));
    }

    #[test]
    fn open_cycle_message_formats_phase_detail() {
        let message = open_cycle_message(OpenCycleMessage {
            file: "doc.md",
            cycle_id: "cycle-4",
            phase: CyclePhase::ResponseCaptured,
            last_event: "capture_response",
            ipc_hint: "",
        });

        assert!(message.contains("response was captured but no write/commit followed"));
    }

    #[test]
    fn open_cycle_manual_patchback_message_formats_recovery() {
        let message = open_cycle_manual_patchback_message(OpenCycleManualPatchbackMessage {
            file: "doc.md",
            cycle_id: "cycle-5",
            phase: CyclePhase::PreflightStarted,
            last_event: "preflight_diff_start",
            marker: "### Re: answer",
        });

        assert!(message.contains("found visible response patchback ### Re: answer"));
        assert!(message.contains("agent-doc write --commit doc.md"));
    }
}

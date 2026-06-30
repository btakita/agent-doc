use std::fmt::Display;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextClearSubmitStatus {
    Accepted,
    TimedOut,
    CaptureFailed,
}

impl ContextClearSubmitStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::TimedOut => "command_still_visible",
            Self::CaptureFailed => "capture_failed",
        }
    }

    pub const fn issue(self) -> Option<&'static str> {
        match self {
            Self::TimedOut => Some("prompt_not_submitted"),
            Self::CaptureFailed => Some("submit_unverified_capture_failed"),
            Self::Accepted => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextClearSubmitObservation {
    pub status: ContextClearSubmitStatus,
    pub elapsed: Duration,
    pub command_visible: bool,
}

pub fn context_clear_command_visible_in_active_input(
    content: &str,
    command: &str,
    is_dispatch_ready_prompt_line: impl Fn(&str) -> bool,
) -> bool {
    let recent_lines: Vec<String> = content
        .lines()
        .rev()
        .take(8)
        .map(crate::prompt::strip_ansi)
        .collect();
    let lines: Vec<&String> = recent_lines.iter().rev().collect();
    for start in 0..lines.len() {
        if !line_shows_context_clear_command_input(lines[start], command) {
            continue;
        }
        let later_has_idle_prompt = lines.iter().skip(start + 1).any(|line| {
            is_dispatch_ready_prompt_line(line.trim())
                || line_starts_with_context_clear_prompt_prefix(line)
        });
        if later_has_idle_prompt {
            continue;
        }
        return true;
    }
    false
}

pub fn context_clear_submit_needs_enter_resubmit(
    observation: &ContextClearSubmitObservation,
    pending_draft_enter_resubmit: bool,
) -> bool {
    pending_draft_enter_resubmit
        && observation.status == ContextClearSubmitStatus::TimedOut
        && observation.command_visible
}

pub fn context_clear_submit_observation_line(
    file: impl Display,
    pane: &str,
    harness: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
    capture_len: Option<usize>,
    capture_hash: Option<&str>,
) -> String {
    let mut line = format!(
        "session_clear_submit_observation file={} pane={} harness={} phase={} result={} elapsed_ms={} command_visible={}",
        file,
        pane,
        harness,
        phase,
        observation.status.as_str(),
        observation.elapsed.as_millis(),
        observation.command_visible
    );
    if let Some(capture_len) = capture_len {
        line.push_str(&format!(" capture_len={capture_len}"));
    }
    if let Some(capture_hash) = capture_hash {
        line.push_str(&format!(" capture_hash={capture_hash}"));
    }
    if let Some(issue) = observation.status.issue() {
        line.push_str(&format!(" issue={issue}"));
    }
    line
}

pub fn context_clear_submit_resubmit_proof_line(
    file: impl Display,
    pane: &str,
    harness: &str,
    submit_key: &str,
    observation: ContextClearSubmitObservation,
) -> String {
    let result = match observation.status {
        ContextClearSubmitStatus::Accepted => "accepted",
        ContextClearSubmitStatus::TimedOut => "still_visible",
        ContextClearSubmitStatus::CaptureFailed => "capture_failed",
    };
    format!(
        "session_clear_submit_resubmit file={} pane={} harness={} action=submit_key key={} result={} elapsed_ms={}",
        file,
        pane,
        harness,
        submit_key,
        result,
        observation.elapsed.as_millis()
    )
}

pub fn context_clear_submit_blocked_line(
    file: impl Display,
    pane: &str,
    harness: &str,
    command: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
) -> String {
    format!(
        "session_clear_submit_blocked file={} pane={} harness={} phase={} command={} result={} elapsed_ms={} command_visible={} issue={} ui_outcome_contract=ui-outcome-v1 ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=restore_idle_prompt_and_retry unblocker=clear_command_not_consumed",
        file,
        pane,
        harness,
        phase,
        command,
        observation.status.as_str(),
        observation.elapsed.as_millis(),
        observation.command_visible,
        observation.status.issue().unwrap_or("submit_not_accepted")
    )
}

pub fn context_clear_submit_blocked_message(
    file: impl Display,
    pane: &str,
    harness: &str,
    command: &str,
    phase: &str,
    observation: ContextClearSubmitObservation,
) -> String {
    format!(
        "session_clear {harness} command `{command}` for {} was not proven submitted in pane {pane} after {phase} (result={}, command_visible={}); treating Clear Session Context as not submitted. ui_outcome=blocked_with_exact_unblocker ui_outcome_class=blocked next_action=restore_idle_prompt_and_retry unblocker=clear_command_not_consumed. Restore an idle {harness} prompt or restart the session, then run Clear Session Context again",
        file,
        observation.status.as_str(),
        observation.command_visible
    )
}

fn line_shows_context_clear_command_input(line: &str, command: &str) -> bool {
    let trimmed = line.trim();
    context_clear_command_candidate_visible(trimmed, command)
        || context_clear_command_candidate_visible(
            strip_context_clear_prompt_prefix(trimmed).trim(),
            command,
        )
}

fn context_clear_command_candidate_visible(candidate: &str, command: &str) -> bool {
    if candidate == command {
        return true;
    }
    if command == "/new" && matches!(candidate, "New session" | "session_new") {
        return true;
    }
    command == "/new"
        && candidate
            .strip_prefix("/new")
            .map(|rest| {
                let label = rest.trim_start();
                label.starts_with("New session") || label.starts_with("session_new")
            })
            .unwrap_or(false)
}

fn line_starts_with_context_clear_prompt_prefix(line: &str) -> bool {
    matches!(line.trim_start().chars().next(), Some('>' | '›' | '❯'))
}

fn strip_context_clear_prompt_prefix(line: &str) -> &str {
    line.trim_start_matches(|ch: char| matches!(ch, '>' | '›' | '❯') || ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_dispatch_ready_prompt_line(line: &str) -> bool {
        matches!(line.trim(), ">" | "›" | "❯")
    }

    #[test]
    fn context_clear_command_visible_detects_codex_active_composer() {
        let content = concat!(
            "older output\n",
            "› /clear\n",
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n",
        );

        assert!(context_clear_command_visible_in_active_input(
            content,
            "/clear",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_command_visible_treats_empty_composer_as_submitted() {
        let content = concat!(
            "older output\n",
            "› Ask Codex to do anything\n",
            "gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used\n",
        );

        assert!(!context_clear_command_visible_in_active_input(
            content,
            "/clear",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_command_visible_detects_opencode_new_palette_row() {
        let content = concat!(
            "older output\n",
            "/new        New session\n",
            "/models     Select model\n",
            "> /new\n",
        );

        assert!(context_clear_command_visible_in_active_input(
            content,
            "/new",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_command_visible_detects_opencode_selected_new_session_command() {
        let content = concat!(
            "older output\n",
            "> New session\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );

        assert!(
            context_clear_command_visible_in_active_input(
                content,
                "/new",
                is_dispatch_ready_prompt_line,
            ),
            "OpenCode can replace `/new` with the selected command label before the final submit Enter"
        );

        let structured = concat!(
            "older output\n",
            "> session_new\n",
            "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n",
        );
        assert!(
            context_clear_command_visible_in_active_input(
                structured,
                "/new",
                is_dispatch_ready_prompt_line,
            ),
            "OpenCode can also surface the selected command id before submission"
        );
    }

    #[test]
    fn context_clear_command_visible_ignores_stale_scrollback_before_idle_prompt() {
        let content = concat!(
            "✶ Generating... (3s · esc to interrupt)\n",
            "  ❯ /clear\n",
            "────────────────────\n",
            "❯ Press up to edit queued messages\n",
            "────────────────────\n",
            "  Opus 4.8 ctx:10% ~/work/btakita/agent-loop main brian@host\n",
            "  bypass permissions on (shift+tab to cycle)\n",
        );

        assert!(!context_clear_command_visible_in_active_input(
            content,
            "/clear",
            is_dispatch_ready_prompt_line,
        ));
    }

    #[test]
    fn context_clear_submit_retry_is_scoped_to_visible_enter_profile_drafts() {
        let visible_timeout = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::TimedOut,
            elapsed: Duration::from_millis(250),
            command_visible: true,
        };
        let accepted = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::Accepted,
            elapsed: Duration::from_millis(20),
            command_visible: false,
        };
        let stale_or_empty_timeout = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::TimedOut,
            elapsed: Duration::from_millis(250),
            command_visible: false,
        };

        assert!(context_clear_submit_needs_enter_resubmit(
            &visible_timeout,
            true,
        ));
        assert!(!context_clear_submit_needs_enter_resubmit(
            &visible_timeout,
            false,
        ));
        assert!(!context_clear_submit_needs_enter_resubmit(&accepted, true));
        assert!(!context_clear_submit_needs_enter_resubmit(
            &stale_or_empty_timeout,
            true,
        ));
    }

    #[test]
    fn context_clear_submit_proof_lines_report_prompt_issue_and_retry_outcome() {
        let observation = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::TimedOut,
            elapsed: Duration::from_millis(5123),
            command_visible: true,
        };
        let issue = context_clear_submit_observation_line(
            "/tmp/doc.md",
            "%7",
            "codex",
            "direct_pane_acceptance",
            observation,
            Some(2048),
            Some("abc123"),
        );
        assert!(
            issue.contains("session_clear_submit_observation"),
            "{issue}"
        );
        assert!(issue.contains("issue=prompt_not_submitted"), "{issue}");
        assert!(issue.contains("command_visible=true"), "{issue}");

        let retry = context_clear_submit_resubmit_proof_line(
            "/tmp/doc.md",
            "%7",
            "codex",
            "Enter",
            ContextClearSubmitObservation {
                status: ContextClearSubmitStatus::Accepted,
                elapsed: Duration::from_millis(150),
                command_visible: false,
            },
        );
        assert!(retry.contains("session_clear_submit_resubmit"), "{retry}");
        assert!(retry.contains("action=submit_key key=Enter"), "{retry}");
        assert!(retry.contains("result=accepted"), "{retry}");
    }

    #[test]
    fn context_clear_submit_blocked_lines_name_command_and_unblocker() {
        let observation = ContextClearSubmitObservation {
            status: ContextClearSubmitStatus::TimedOut,
            elapsed: Duration::from_millis(2001),
            command_visible: true,
        };
        let line = context_clear_submit_blocked_line(
            "/tmp/doc.md",
            "%12",
            "opencode",
            "/new",
            "direct_pane_resubmit_acceptance",
            observation,
        );
        assert!(line.contains("session_clear_submit_blocked"), "{line}");
        assert!(line.contains("command=/new"), "{line}");
        assert!(
            line.contains("ui_outcome=blocked_with_exact_unblocker"),
            "{line}"
        );
        assert!(
            line.contains("unblocker=clear_command_not_consumed"),
            "{line}"
        );

        let message = context_clear_submit_blocked_message(
            "/tmp/doc.md",
            "%12",
            "opencode",
            "/new",
            "direct_pane_resubmit_acceptance",
            observation,
        );
        assert!(message.contains("command `/new`"), "{message}");
        assert!(
            message.contains("treating Clear Session Context as not submitted"),
            "{message}"
        );
        assert!(
            message.contains("ui_outcome=blocked_with_exact_unblocker"),
            "{message}"
        );
    }
}

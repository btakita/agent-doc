//! Grok Build boundary observations, verified against the 1.0.24 fullscreen TUI.
//! Existing lifecycle computed projections consume these pure observations.
use super::{PaneComposerProjection, PaneComposerReadinessEvidence};

pub fn blocker(output: &str) -> Option<String> {
    let recent: Vec<_> = output
        .lines()
        .rev()
        .take(32)
        .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
        .map(|line| line.trim().to_ascii_lowercase())
        .collect();
    if recent
        .iter()
        .any(|line| line.contains("ctrl+c:cancel") || line.ends_with("[stop]"))
    {
        return Some("active Grok Build turn".into());
    }
    None
}

pub fn project(output: &str) -> PaneComposerProjection {
    let raw_lines: Vec<_> = output.lines().collect();
    let lines: Vec<_> = raw_lines
        .iter()
        .map(|line| agent_doc_turn_executor_tmux::prompt::strip_ansi(line))
        .collect();
    let Some(start) = lines
        .iter()
        .rposition(|line| line.trim().starts_with('╭') && line.trim().ends_with('╮'))
    else {
        return PaneComposerProjection::Absent;
    };
    let Some(first) = lines
        .get(start + 1)
        .and_then(|line| line.trim().strip_prefix("│ ❯"))
    else {
        return PaneComposerProjection::Absent;
    };
    let Some(first) = first.strip_suffix('│') else {
        return PaneComposerProjection::Absent;
    };
    let mut draft = first.trim().to_string();
    let mut end = None;
    for (i, line) in lines.iter().enumerate().skip(start + 2) {
        let line = line.trim();
        if line.starts_with('╰') && line.ends_with('╯') {
            end = Some(i);
            break;
        }
        let Some(body) = line.strip_prefix('│').and_then(|s| s.strip_suffix('│')) else {
            return PaneComposerProjection::Absent;
        };
        if !body.trim().is_empty() {
            if !draft.is_empty() {
                draft.push('\n');
            }
            draft.push_str(body.trim());
        }
    }
    let Some(end) = end else {
        return PaneComposerProjection::Absent;
    };
    // Grok Build's next-prompt suggestion is visual ghost text, not textarea
    // state. Terminal-native themes mark it faint+italic; RGB themes use a
    // light-gray foreground+italic. Preserve the raw ANSI until this boundary:
    // stripping it first makes the ghost indistinguishable from typed input and
    // strands dispatch-only reopen behind `submit_or_clear_pane_draft`.
    let first_body_is_ghost = raw_lines.get(start + 1).is_some_and(|line| {
        agent_doc_turn_executor_tmux::prompt::prompt_line_body_is_ghost_placeholder(line)
    });
    if !draft.is_empty() && !first_body_is_ghost {
        return PaneComposerProjection::OperatorDraft { preview: draft };
    }
    // Only the observed idle footer may follow the box. Unknown modal/help/auth
    // UI remains absent, even if old prompt artwork is still visible above it.
    if lines[end + 1..].iter().any(|line| {
        let line = line.trim();
        !line.is_empty()
            && line != "[stable]"
            && line != "[alpha]"
            && !(line.starts_with("Shift+Tab:mode")
                && line.ends_with("Ctrl+x:shortcuts")
                && !line.contains("cancel"))
    }) {
        return PaneComposerProjection::Absent;
    }
    PaneComposerProjection::ReadyEmpty {
        evidence: PaneComposerReadinessEvidence::Prompt {
            rendered: lines[start + 1].trim().into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HarnessConfig, project_pane_composer};
    const IDLE: &str = "Grok Build 1.0.24\n╭────────────────────────╮\n│ ❯                      │\n╰──── Grok 4.6 (high) ───╯\nShift+Tab:mode  │  Ctrl+x:shortcuts\n";

    #[test]
    fn launch_preserves_exact_lineage_and_does_not_fall_back_to_claude() {
        let h = HarnessConfig::from_agent_name("grok-build");
        assert_eq!(h.binary, "grok");
        assert!(h.is_tui_harness());
        assert_eq!(h.trigger_command("notes.md"), "agent-doc notes.md");
        let args = h
            .exact_resume_args(
                &[
                    "--model".into(),
                    "custom".into(),
                    "--session-id".into(),
                    "fresh-id".into(),
                ],
                "bound-id",
            )
            .unwrap()
            .unwrap();
        assert_eq!(args, ["--model", "custom", "--resume", "bound-id"]);
        assert_eq!(h.restart_args(&[]).unwrap(), Vec::<String>::new());
        assert_eq!(crate::operator_quit_key_plan("grok"), vec!["C-q", "C-q"]);
        assert!(h.is_dispatch_ready_prompt_line(&h.last_prompt_candidate(IDLE).unwrap()));
    }

    #[test]
    fn composer_proof_is_structural_and_preserves_drafts() {
        let h = HarnessConfig::grok();
        assert!(matches!(
            project_pane_composer(IDLE, &h),
            PaneComposerProjection::ReadyEmpty { .. }
        ));
        assert!(matches!(
            project_pane_composer(&IDLE.replace("❯ ", "❯ unfinished"), &h),
            PaneComposerProjection::OperatorDraft { .. }
        ));
        for output in [
            "$ grok\n❯",
            "Usage: grok\n>",
            "Sign in to Grok\n❯",
            "╭──╮\n│ ❯ │",
            "",
        ] {
            assert_eq!(
                project_pane_composer(output, &h),
                PaneComposerProjection::Absent,
                "{output}"
            );
        }
        assert_eq!(
            project_pane_composer(&format!("{IDLE}Authorize this tool?"), &h),
            PaneComposerProjection::Absent
        );
        assert_eq!(
            project_pane_composer(&IDLE.replace("Ctrl+x", "Ctrl+c:cancel │ Ctrl+x"), &h),
            PaneComposerProjection::Busy
        );
        assert_eq!(
            project_pane_composer(&format!("Waiting for response… 1.0s [stop]\n{IDLE}"), &h),
            PaneComposerProjection::Busy
        );
    }

    #[test]
    fn next_prompt_ghost_is_ready_empty_but_same_visible_typed_text_is_a_draft() {
        let h = HarnessConfig::grok();
        let visible = "agent-doc tasks/sdk/sdk-pr.md";

        // GrokNight and other RGB themes render predicted next-prompt text as
        // light gray + italic, without SGR faint.
        let rgb_ghost = IDLE.replace(
            "│ ❯                      │",
            &format!(
                "\x1b[38;2;88;88;88m│\x1b[0m \x1b[38;2;122;162;247m❯\x1b[0m \
                 \x1b[3;38;2;88;88;88m{visible}\x1b[0m │"
            ),
        );
        assert!(matches!(
            project_pane_composer(&rgb_ghost, &h),
            PaneComposerProjection::ReadyEmpty { .. }
        ));

        // The terminal-native theme uses faint+italic for the same ghost.
        let terminal_ghost = IDLE.replace(
            "│ ❯                      │",
            &format!("│ ❯ \x1b[2;3m{visible}\x1b[0m │"),
        );
        assert!(matches!(
            project_pane_composer(&terminal_ghost, &h),
            PaneComposerProjection::ReadyEmpty { .. }
        ));

        // Color alone is not enough. The same visible text at normal intensity
        // is committed operator input and must remain protected.
        let typed = IDLE.replace(
            "│ ❯                      │",
            &format!("│ ❯ \x1b[38;2;192;202;245m{visible}\x1b[0m │"),
        );
        assert_eq!(
            project_pane_composer(&typed, &h),
            PaneComposerProjection::OperatorDraft {
                preview: visible.to_string(),
            },
        );
    }
}

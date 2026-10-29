//! Grok Build boundary observations, verified against the fullscreen TUI.
//! Existing lifecycle computed projections consume these pure observations.
//!
//! Grok's composer footer is part of readiness evidence. It may include the
//! suggestion controls and dashboard status row introduced after 1.0.24, but
//! unknown post-composer UI remains fail-closed.
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

fn is_idle_footer_line(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || line == "[stable]" || line == "[alpha]" {
        return true;
    }

    let controls = (line.starts_with("Shift+Tab:mode")
        || line.starts_with("Tab/→:accept suggestion"))
        && line.ends_with("Ctrl+x:shortcuts")
        && !line.contains("cancel");
    if controls {
        return true;
    }

    // Grok 4.7 adds a single status row below the controls. The document/model
    // label and cwd are intentionally free-form; the numeric context fraction
    // followed by the Dashboard affordance is the stable structural chrome.
    let Some((status, dashboard)) = line.rsplit_once('│') else {
        return false;
    };
    if dashboard.trim() != "[Dashboard]" {
        return false;
    }
    let mut tokens = status.split_whitespace().rev();
    let (Some(limit), Some("/"), Some(used)) = (tokens.next(), tokens.next(), tokens.next()) else {
        return false;
    };
    [used, limit].into_iter().all(|quantity| {
        let numeric = quantity
            .strip_suffix('K')
            .or_else(|| quantity.strip_suffix('M'))
            .unwrap_or(quantity);
        !numeric.is_empty()
            && numeric.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            && numeric.chars().filter(|ch| *ch == '.').count() <= 1
    })
}

struct ParsedGrokComposer {
    draft: String,
    ghost: bool,
    end: usize,
    lines: Vec<String>,
    rendered_prompt: String,
}

/// The boxed composer, independent of whether the footer is an idle prompt or
/// a live `Ctrl+c:cancel` turn. `None` when the box cannot be read.
fn parse_grok_composer(output: &str) -> Option<ParsedGrokComposer> {
    let raw_lines: Vec<_> = output.lines().collect();
    let lines: Vec<String> = raw_lines
        .iter()
        .map(|line| agent_doc_turn_executor_tmux::prompt::strip_ansi(line))
        .collect();
    let start = lines.iter().rposition(|line| {
        let line = line.trim();
        line.starts_with('╭') && line.ends_with('╮')
    })?;
    let first = lines.get(start + 1)?.trim().strip_prefix("│ ❯")?;
    let first = first.strip_suffix('│')?;
    let mut draft = first.trim().to_string();
    let mut end = None;
    for (i, line) in lines.iter().enumerate().skip(start + 2) {
        let line = line.trim();
        if line.starts_with('╰') && line.ends_with('╯') {
            end = Some(i);
            break;
        }
        let body = line
            .strip_prefix('│')
            .and_then(|text| text.strip_suffix('│'))?;
        if !body.trim().is_empty() {
            if !draft.is_empty() {
                draft.push('\n');
            }
            draft.push_str(body.trim());
        }
    }
    let end = end?;
    // Grok Build's next-prompt suggestion is visual ghost text, not textarea
    // state. Terminal-native themes mark it faint+italic; RGB themes use a
    // light-gray foreground+italic. Preserve the raw ANSI until this boundary:
    // stripping it first makes the ghost indistinguishable from typed input and
    // strands dispatch-only reopen behind `submit_or_clear_pane_draft`.
    let ghost = raw_lines.get(start + 1).is_some_and(|line| {
        agent_doc_turn_executor_tmux::prompt::prompt_line_body_is_ghost_placeholder(line)
    });
    let rendered_prompt = lines[start + 1].trim().to_string();
    Some(ParsedGrokComposer {
        draft,
        ghost,
        end,
        lines,
        rendered_prompt,
    })
}

pub fn project(output: &str) -> PaneComposerProjection {
    let Some(parsed) = parse_grok_composer(output) else {
        return PaneComposerProjection::Absent;
    };
    if !parsed.draft.is_empty() && !parsed.ghost {
        return PaneComposerProjection::OperatorDraft {
            preview: parsed.draft,
        };
    }
    // Only the observed idle footer may follow the box. Unknown modal/help/auth
    // UI remains absent, even if old prompt artwork is still visible above it.
    if parsed.lines[parsed.end + 1..]
        .iter()
        .any(|line| !is_idle_footer_line(line))
    {
        return PaneComposerProjection::Absent;
    }
    PaneComposerProjection::ReadyEmpty {
        evidence: PaneComposerReadinessEvidence::Prompt {
            rendered: parsed.rendered_prompt,
        },
    }
}

/// Whether a running Grok composer can take one mid-turn follow-up
/// (`#routesyncsteer`). A busy footer still parses: the readiness projection
/// collapses that screen to `Busy` and would otherwise hide an operator draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowUpComposer {
    /// Empty box, or ghost suggestion text only.
    Empty,
    /// Unsent operator text. A follow-up must not clobber it.
    Draft { preview: String },
    /// The box could not be read. Do not type.
    Unreadable,
}

pub fn follow_up_composer(output: &str) -> FollowUpComposer {
    let Some(parsed) = parse_grok_composer(output) else {
        return FollowUpComposer::Unreadable;
    };
    if !parsed.draft.is_empty() && !parsed.ghost {
        FollowUpComposer::Draft {
            preview: parsed.draft,
        }
    } else {
        FollowUpComposer::Empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HarnessConfig, project_pane_composer};
    const IDLE: &str = "Grok Build 1.0.24\n╭────────────────────────╮\n│ ❯                      │\n╰──── Grok 4.6 (high) ───╯\nShift+Tab:mode  │  Ctrl+x:shortcuts\n";
    const LIVE_GROK_47_IDLE_GHOST: &str = concat!(
        "Help improve Grok                    [Opt out] [Opt in]\n",
        "Off by default. Opt-in to allow SpaceXAI to retain coding data.\n",
        "╭────────────────────────╮\n",
        "│ ❯ \x1b[3;38;2;88;88;88magent-doc tasks/sdk/sdk-pr.md\x1b[0m │\n",
        "╰──── Grok 4.7 (high) ───╯\n",
        "Tab/→:accept suggestion  │  Shift+Tab:mode  │  Ctrl+x:shortcuts\n",
        "docs/fpe-service-topology-temporal-plan ~/w/b/a/src/haiven-dev  93K / 256K │ [Dashboard]\n",
    );

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
        let busy = IDLE.replace("Ctrl+x", "Ctrl+c:cancel │ Ctrl+x");
        assert_eq!(follow_up_composer(IDLE), FollowUpComposer::Empty);
        assert_eq!(follow_up_composer(&busy), FollowUpComposer::Empty);
        assert_eq!(
            follow_up_composer(&busy.replace("│ ❯ ", "│ ❯ unfinished note ")),
            FollowUpComposer::Draft {
                preview: "unfinished note".to_string(),
            }
        );
        assert_eq!(
            follow_up_composer("no composer"),
            FollowUpComposer::Unreadable
        );
        assert_eq!(
            follow_up_composer(LIVE_GROK_47_IDLE_GHOST),
            FollowUpComposer::Empty
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

    #[test]
    fn live_grok_47_suggestion_footer_is_dispatch_ready() {
        let h = HarnessConfig::grok();
        assert!(matches!(
            project_pane_composer(LIVE_GROK_47_IDLE_GHOST, &h),
            PaneComposerProjection::ReadyEmpty { .. }
        ));

        let typed =
            LIVE_GROK_47_IDLE_GHOST.replace("\x1b[3;38;2;88;88;88m", "\x1b[38;2;192;202;245m");
        assert!(matches!(
            project_pane_composer(&typed, &h),
            PaneComposerProjection::OperatorDraft { .. }
        ));
    }

    #[test]
    fn suggestion_footer_keeps_unknown_post_composer_ui_fail_closed() {
        let h = HarnessConfig::grok();
        let unknown = format!("{LIVE_GROK_47_IDLE_GHOST}Authorize this tool?\n");
        assert_eq!(
            project_pane_composer(&unknown, &h),
            PaneComposerProjection::Absent
        );

        let fake_dashboard = LIVE_GROK_47_IDLE_GHOST
            .replace("93K / 256K │ [Dashboard]", "Authorize / Deny │ [Dashboard]");
        assert_eq!(
            project_pane_composer(&fake_dashboard, &h),
            PaneComposerProjection::Absent
        );
    }
}

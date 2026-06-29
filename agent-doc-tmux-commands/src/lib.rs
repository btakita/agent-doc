//! Pure tmux command builders and output parsers.
//!
//! This crate builds argv vectors and parses command output. It does not spawn
//! processes or decide turn lifecycle actions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxCommand {
    args: Vec<String>,
}

impl TmuxCommand {
    pub fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn into_args(self) -> Vec<String> {
        self.args
    }
}

pub fn display_message(target: Option<&str>, format: &str) -> TmuxCommand {
    let mut args = vec!["display-message".to_string(), "-p".to_string()];
    push_optional_target(&mut args, target);
    args.push(format.to_string());
    TmuxCommand::new(args)
}

pub fn list_panes(target: Option<&str>, format: &str) -> TmuxCommand {
    let mut args = vec!["list-panes".to_string()];
    push_optional_target(&mut args, target);
    args.extend(["-F".to_string(), format.to_string()]);
    TmuxCommand::new(args)
}

pub fn capture_pane(target: &str) -> TmuxCommand {
    TmuxCommand::new(["capture-pane", "-p", "-t", target])
}

pub fn send_keys_literal(target: &str, text: &str) -> TmuxCommand {
    TmuxCommand::new(["send-keys", "-t", target, "-l", text])
}

pub fn send_key(target: &str, key: &str) -> TmuxCommand {
    TmuxCommand::new(["send-keys", "-t", target, key])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmuxSubmitProfile {
    /// When non-zero, send the text and the submit key as separate
    /// `tmux send-keys` calls with this delay between them.
    ///
    /// OpenCode's slash-command palette opens the moment `/` is typed. If Enter
    /// is sent in the same `tmux send-keys` call as the text, the palette can
    /// swallow the Enter instead of submitting the composer. Splitting the send
    /// gives the TUI time to settle. Other harnesses keep the canonical
    /// single-call form.
    split_text_and_submit_delay_ms: u64,
}

impl TmuxSubmitProfile {
    pub const fn new() -> Self {
        Self {
            split_text_and_submit_delay_ms: 0,
        }
    }

    pub const fn with_split_text_submit_delay(delay_ms: u64) -> Self {
        Self {
            split_text_and_submit_delay_ms: delay_ms,
        }
    }

    pub const fn mode(self) -> &'static str {
        "tmux_text_enter"
    }

    pub const fn transform(self) -> &'static str {
        "tmux_text_enter"
    }

    pub const fn submit_key(self) -> &'static str {
        "Enter"
    }

    pub const fn pending_draft_enter_resubmit(self) -> bool {
        true
    }

    pub const fn split_text_and_submit_delay_ms(self) -> u64 {
        self.split_text_and_submit_delay_ms
    }
}

impl Default for TmuxSubmitProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// OpenCode needs the split text+Enter send because its slash-command palette
/// opens on `/` and swallows a same-call Enter. This is `const fn`-safe byte
/// compare because `str` equality is not const.
const fn harness_is_opencode(harness: &str) -> bool {
    let b = harness.as_bytes();
    b.len() == 8
        && b[0] == b'o'
        && b[1] == b'p'
        && b[2] == b'e'
        && b[3] == b'n'
        && b[4] == b'c'
        && b[5] == b'o'
        && b[6] == b'd'
        && b[7] == b'e'
}

pub const fn tmux_submit_profile_for_harness(harness: &str) -> TmuxSubmitProfile {
    if harness_is_opencode(harness) {
        TmuxSubmitProfile::with_split_text_submit_delay(80)
    } else {
        TmuxSubmitProfile::new()
    }
}

pub const fn tmux_submit_mode_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).mode()
}

pub const fn tmux_submit_transform_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).transform()
}

pub const fn tmux_submit_key_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).submit_key()
}

pub fn submitted_text_without_trailing_line_endings(text: &str) -> &str {
    text.trim_end_matches(['\r', '\n'])
}

pub fn text_submit_command(target: &str, text: &str, profile: TmuxSubmitProfile) -> TmuxCommand {
    let text = submitted_text_without_trailing_line_endings(text);
    let mut args = vec![
        "send-keys".to_string(),
        "-t".to_string(),
        target.to_string(),
    ];
    if !text.is_empty() {
        args.push(text.to_string());
    }
    args.push(profile.submit_key().to_string());
    TmuxCommand::new(args)
}

/// Arg list for the text-only half of a split send, same shape as
/// [`text_submit_command`] minus the trailing submit key.
pub fn text_only_command(target: &str, text: &str) -> TmuxCommand {
    let text = submitted_text_without_trailing_line_endings(text);
    let mut args = vec![
        "send-keys".to_string(),
        "-t".to_string(),
        target.to_string(),
    ];
    if !text.is_empty() {
        args.push(text.to_string());
    }
    TmuxCommand::new(args)
}

pub fn parse_nonempty_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn push_optional_target(args: &mut Vec<String>, target: Option<&str>) {
    if let Some(target) = target {
        args.extend(["-t".to_string(), target.to_string()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_send_keys_keeps_user_text_as_one_arg() {
        let command = send_keys_literal("%1", "--hello world");

        assert_eq!(
            command.args(),
            ["send-keys", "-t", "%1", "-l", "--hello world"]
        );
    }

    #[test]
    fn list_panes_places_target_before_format() {
        let command = list_panes(Some(":agent"), "#{pane_id}");

        assert_eq!(
            command.args(),
            ["list-panes", "-t", ":agent", "-F", "#{pane_id}"]
        );
    }

    #[test]
    fn parser_discards_blank_lines() {
        assert_eq!(
            parse_nonempty_lines("\n%1\n  \n%2  \n"),
            vec!["%1".to_string(), "%2".to_string()]
        );
    }

    #[test]
    fn submit_profiles_keep_harness_submit_policy_in_one_place() {
        for harness in ["codex", "claude", "opencode", "unknown-harness"] {
            assert_eq!(tmux_submit_mode_for_harness(harness), "tmux_text_enter");
            assert_eq!(
                tmux_submit_transform_for_harness(harness),
                "tmux_text_enter"
            );
            assert_eq!(tmux_submit_key_for_harness(harness), "Enter");
            assert_eq!(
                submitted_text_without_trailing_line_endings("agent-doc plan.md\r\n"),
                "agent-doc plan.md"
            );
            assert_eq!(
                text_submit_command(
                    "%7",
                    "agent-doc plan.md\r\n",
                    tmux_submit_profile_for_harness(harness)
                )
                .into_args(),
                ["send-keys", "-t", "%7", "agent-doc plan.md", "Enter"]
                    .into_iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "{harness} must submit tmux text with one named Enter key"
            );
            assert_eq!(
                text_submit_command("%7", "\n", tmux_submit_profile_for_harness(harness))
                    .into_args(),
                ["send-keys", "-t", "%7", "Enter"]
                    .into_iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "{harness} empty resubmit must send only the named Enter key"
            );
        }
    }

    #[test]
    fn tmux_submit_profile_splits_text_and_enter_only_for_opencode() {
        let opencode = tmux_submit_profile_for_harness("opencode");
        assert!(
            opencode.split_text_and_submit_delay_ms() > 0,
            "opencode must request a split text+Enter send so the slash-command palette can settle before the Enter arrives"
        );
        assert_eq!(opencode.submit_key(), "Enter");
        assert_eq!(opencode.mode(), "tmux_text_enter");
        assert_eq!(opencode.transform(), "tmux_text_enter");
        assert!(opencode.pending_draft_enter_resubmit());

        for non_opencode in ["codex", "claude", "claude-code", "default", "", "unknown"] {
            let profile = tmux_submit_profile_for_harness(non_opencode);
            assert_eq!(
                profile.split_text_and_submit_delay_ms(),
                0,
                "{non_opencode:?} must keep the single-call text+Enter send (no split)"
            );
        }
    }

    #[test]
    fn text_only_command_omits_submit_key_for_split_send() {
        assert_eq!(
            text_only_command("%7", "/new").into_args(),
            ["send-keys", "-t", "%7", "/new"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            "split-send text step must not include the trailing Enter"
        );
        assert_eq!(
            text_only_command("%7", "/new\r\n").into_args(),
            ["send-keys", "-t", "%7", "/new"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            "split-send text step must strip trailing line endings before sending"
        );
        assert_eq!(
            text_only_command("%7", "\n").into_args(),
            ["send-keys", "-t", "%7"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
    }
}

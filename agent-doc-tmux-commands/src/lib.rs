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
}

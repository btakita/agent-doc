//! Supervisor start command rendering.
//!
//! Process ownership lives in this crate. Callers provide already-resolved
//! binary and document facts; this module only renders the shell command to
//! submit into a preserved route-owned pane.

use std::path::Path;

fn shell_quote_arg(raw: &str) -> String {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '+'))
    {
        return raw.to_string();
    }
    format!("'{}'", raw.replace('\'', "'\\''"))
}

pub fn route_owned_start_command(agent_doc_bin: &str, file: &Path) -> String {
    format!(
        "{} start --route-owned {}",
        shell_quote_arg(agent_doc_bin),
        shell_quote_arg(&file.to_string_lossy())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_owned_start_command_preserves_safe_args_unquoted() {
        assert_eq!(
            route_owned_start_command("/usr/local/bin/agent-doc", Path::new("tasks/doc.md")),
            "/usr/local/bin/agent-doc start --route-owned tasks/doc.md"
        );
    }

    #[test]
    fn route_owned_start_command_quotes_spaces() {
        assert_eq!(
            route_owned_start_command("/tmp/agent doc", Path::new("tasks/my doc.md")),
            "'/tmp/agent doc' start --route-owned 'tasks/my doc.md'"
        );
    }

    #[test]
    fn route_owned_start_command_quotes_empty_args() {
        assert_eq!(
            route_owned_start_command("", Path::new("")),
            "'' start --route-owned ''"
        );
    }

    #[test]
    fn route_owned_start_command_escapes_single_quotes() {
        assert_eq!(
            route_owned_start_command("/tmp/agent'doc", Path::new("tasks/brian's doc.md")),
            "'/tmp/agent'\\''doc' start --route-owned 'tasks/brian'\\''s doc.md'"
        );
    }
}

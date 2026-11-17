//! A bare session invocation is transport, not queue work.
//!
//! `agent-doc <file>` and `/agent-doc <file>` start the harness session.
//! `claim`, `compact`, and any trailing body are real prompts.

use agent_doc_document::queue_projection::strip_priority_markers;

/// True when `text` is only a session invocation, after queue presentation
/// (`### `, pins, list markers, lifecycle markers, stable `[#q-…]` ids).
pub fn is_session_trigger_prompt(text: &str) -> bool {
    let Some(line) = bare_trigger_line(text) else {
        return false;
    };
    let mut tokens = line.split_whitespace();
    let Some(command) = tokens.next() else {
        return false;
    };
    if command != "agent-doc" && command != "/agent-doc" {
        return false;
    }
    let Some(file) = tokens.next() else {
        return false;
    };
    if tokens.next().is_some() {
        return false;
    }
    looks_like_session_document_path(unquote(file))
}

fn bare_trigger_line(text: &str) -> Option<String> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let first = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    let mut line = first.trim();
    if let Some(inner) = line
        .strip_prefix("~~")
        .and_then(|rest| rest.strip_suffix("~~"))
    {
        line = inner.trim();
    }
    loop {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            line = rest;
            continue;
        }
        if let Some(dot) = trimmed.find(". ") {
            let digits = &trimmed[..dot];
            if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
                line = &trimmed[dot + 2..];
                continue;
            }
        }
        let without_heading = crate::document_queue::strip_top_level_queue_heading(trimmed);
        if without_heading != trimmed {
            line = without_heading;
            continue;
        }
        break;
    }
    let stripped = strip_priority_markers(line);
    let stripped = agent_doc_element_queue::strip_queue_prompt_stable_id_annotation(&stripped);
    let stripped = stripped.trim();
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn unquote(token: &str) -> &str {
    token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            token
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(token)
}

fn looks_like_session_document_path(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    file.contains('/')
        || file.contains('\\')
        || lower.ends_with(".md")
        || lower.ends_with(".markdown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_trigger_matches_bare_invocations_only() {
        assert!(is_session_trigger_prompt(
            "agent-doc /home/brian/work/tasks/bugs.md"
        ));
        assert!(is_session_trigger_prompt(
            "### 📌 agent-doc /home/brian/work/tasks/bugs.md"
        ));
        assert!(is_session_trigger_prompt(
            "- ### 📌 /agent-doc tasks/bugs.md [#q-abc]"
        ));
        assert!(is_session_trigger_prompt("agent-doc tasks/bugs.md"));
        assert!(is_session_trigger_prompt("agent-doc 'tasks/bugs.md'"));
        assert!(is_session_trigger_prompt("🚧 agent-doc bugs.md"));
        assert!(!is_session_trigger_prompt(
            "agent-doc claim /home/brian/work/tasks/bugs.md"
        ));
        assert!(!is_session_trigger_prompt(
            "agent-doc compact /home/brian/work/tasks/bugs.md"
        ));
        assert!(!is_session_trigger_prompt(
            "agent-doc /home/brian/work/tasks/bugs.md #bug"
        ));
        assert!(!is_session_trigger_prompt(
            "agent-doc /home/brian/work/tasks/bugs.md\nfix the parser"
        ));
        assert!(!is_session_trigger_prompt("fix the parser"));
        assert!(!is_session_trigger_prompt(
            "```\nagent-doc /home/brian/work/tasks/bugs.md\n```"
        ));
    }
}

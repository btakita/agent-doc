//! Editor route-error diagnostic naming policy.

pub const EDITOR_ROUTE_ERROR_DIAGNOSTICS_DIR: &str = ".agent-doc/state/editor-route-errors";

pub fn editor_route_error_diagnostic_name(relative_path: &str) -> String {
    let mut sanitized = String::new();
    for ch in relative_path.replace('\\', "/").chars() {
        match ch {
            '/' => sanitized.push_str("__"),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '-' => sanitized.push(ch),
            _ => sanitized.push('_'),
        }
    }
    if sanitized.is_empty() {
        "route-error".to_string()
    } else {
        sanitized
    }
}

pub fn editor_route_error_file_name(relative_path: &str) -> String {
    format!("{}.txt", editor_route_error_diagnostic_name(relative_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_name_matches_editor_sanitization() {
        assert_eq!(
            editor_route_error_diagnostic_name("tasks/agent-doc/agent-doc-bugs2.md"),
            "tasks__agent-doc__agent-doc-bugs2.md"
        );
        assert_eq!(
            editor_route_error_file_name("tasks\\agent doc\\bug?.md"),
            "tasks__agent_doc__bug_.md.txt"
        );
    }

    #[test]
    fn diagnostic_name_falls_back_for_empty_paths() {
        assert_eq!(editor_route_error_diagnostic_name(""), "route-error");
        assert_eq!(editor_route_error_file_name(""), "route-error.txt");
    }
}

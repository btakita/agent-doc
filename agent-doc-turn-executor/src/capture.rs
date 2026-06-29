//! Pure capture-delta policy for turn executor output streams.

/// Extract new lines from a pane capture by diffing against the previous capture.
///
/// The policy compares line-by-line, finds the first divergence point, and returns
/// all lines from that point onward in the new capture.
pub fn capture_delta(previous: &str, current: &str) -> String {
    let previous_lines: Vec<&str> = previous.lines().collect();
    let current_lines: Vec<&str> = current.lines().collect();

    let common_prefix = previous_lines
        .iter()
        .zip(current_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if common_prefix < current_lines.len() {
        current_lines[common_prefix..].join("\n")
    } else {
        String::new()
    }
}

/// Limit captured content to the last `max_lines` lines to prevent unbounded
/// console growth.
pub fn limit_capture_lines(content: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        return content.to_string();
    }
    lines[lines.len() - max_lines..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_delta_returns_appended_lines() {
        let previous = "line 1\nline 2\nline 3";
        let current = "line 1\nline 2\nline 3\nline 4\nline 5";

        assert_eq!(capture_delta(previous, current), "line 4\nline 5");
    }

    #[test]
    fn capture_delta_returns_from_first_modified_line() {
        let previous = "line 1\nline 2\nline 3";
        let current = "line 1\nchanged\nline 3\nline 4";

        assert_eq!(capture_delta(previous, current), "changed\nline 3\nline 4");
    }

    #[test]
    fn capture_delta_returns_empty_for_identical_or_truncated_capture() {
        assert_eq!(capture_delta("line 1\nline 2", "line 1\nline 2"), "");
        assert_eq!(capture_delta("line 1\nline 2", ""), "");
    }

    #[test]
    fn capture_delta_empty_previous_returns_current_capture() {
        assert_eq!(capture_delta("", "line 1\nline 2"), "line 1\nline 2");
    }

    #[test]
    fn limit_capture_lines_keeps_tail_window() {
        assert_eq!(limit_capture_lines("1\n2\n3\n4", 2), "3\n4");
    }

    #[test]
    fn limit_capture_lines_preserves_short_content() {
        assert_eq!(limit_capture_lines("1\n2", 5), "1\n2");
    }
}

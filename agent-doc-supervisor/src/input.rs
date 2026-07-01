//! Pure supervisor input byte policy.
//!
//! The orchestration crate owns PTY reads/writes and prompt visibility facts.
//! This module owns byte-level decisions that are independent of those effects.

pub fn strip_stale_ctrl_d_before_prompt(
    data: &[u8],
    suppress_stale_ctrl_d_until_prompt: bool,
    prompt_visible_once: bool,
) -> Option<Vec<u8>> {
    if !suppress_stale_ctrl_d_until_prompt || prompt_visible_once || !data.contains(&0x04) {
        return None;
    }

    Some(data.iter().copied().filter(|byte| *byte != 0x04).collect())
}

pub fn normalize_supervisor_inject_bytes(bytes: &str) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let raw = bytes.as_bytes();
    let mut index = 0usize;
    while index < raw.len() {
        match raw[index] {
            b'\r' => {
                normalized.push(b'\r');
                if raw.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
            }
            b'\n' => normalized.push(b'\r'),
            byte => normalized.push(byte),
        }
        index += 1;
    }
    normalized
}

pub fn prompt_input_summary(input: &str) -> String {
    let trimmed = input.trim_end_matches(&['\r', '\n'][..]);
    let mut summary = String::new();
    let mut count = 0usize;
    for ch in trimmed.chars() {
        count += 1;
        if count > 32 {
            summary.push_str("...");
            break;
        }
        match ch {
            '\r' => summary.push_str("\\r"),
            '\n' => summary.push_str("\\n"),
            '\t' => summary.push_str("\\t"),
            c if c.is_control() => summary.push('?'),
            c => summary.push(c),
        }
    }
    if summary.is_empty() {
        "<empty>".to_string()
    } else {
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_ctrl_d_before_prompt_drops_inherited_ctrl_d_bytes() {
        let filtered =
            strip_stale_ctrl_d_before_prompt(b"\x04status\x04", true, false).expect("filtered");
        assert_eq!(filtered, b"status");
    }

    #[test]
    fn stale_ctrl_d_before_prompt_keeps_ctrl_d_once_prompt_is_visible() {
        assert!(
            strip_stale_ctrl_d_before_prompt(b"\x04", true, true).is_none(),
            "prompt-visible children should still receive a fresh Ctrl+D"
        );
        assert!(
            strip_stale_ctrl_d_before_prompt(b"\x04", false, false).is_none(),
            "non-keepalive runs should not rewrite forwarded Ctrl+D"
        );
    }

    #[test]
    fn supervisor_inject_bytes_converts_line_feeds_to_carriage_returns() {
        assert_eq!(
            normalize_supervisor_inject_bytes("agent-doc tasks/software/tsift.md\n"),
            b"agent-doc tasks/software/tsift.md\r"
        );
        assert_eq!(
            normalize_supervisor_inject_bytes("line one\r\nline two\nline three\r"),
            b"line one\rline two\rline three\r"
        );
    }

    #[test]
    fn prompt_input_summary_escapes_and_truncates() {
        assert_eq!(prompt_input_summary("\n"), "<empty>");
        assert_eq!(prompt_input_summary("abc\tdef\n"), "abc\\tdef");
        assert_eq!(
            prompt_input_summary("abcdefghijklmnopqrstuvwxyz1234567890\n"),
            "abcdefghijklmnopqrstuvwxyz123456..."
        );
    }
}

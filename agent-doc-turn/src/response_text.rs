//! Pure response text normalization for turn closeout.

/// Strip leading `## Assistant` and trailing `## User` headings from append-mode
/// response text.
///
/// The append writer adds its own `## Assistant` prefix and `## User` suffix, so
/// echoed transcript headings are removed before the response is persisted.
pub fn strip_assistant_heading(response: &str) -> String {
    let mut result = response.to_string();

    let trimmed = result.trim_start();
    if let Some(rest) = trimmed.strip_prefix("## Assistant") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        result = rest.to_string();
    }

    let trimmed_end = result.trim_end();
    if let Some(before) = trimmed_end.strip_suffix("## User") {
        result = before.trim_end_matches('\n').to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_leading_space_before_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("\n\n## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_trailing_user_heading_and_keeps_newline() {
        assert_eq!(
            strip_assistant_heading("Done.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn strips_both_echoed_headings() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn leaves_plain_response_unchanged() {
        let response = "Done.\n\nDetails.";
        assert_eq!(strip_assistant_heading(response), response.to_string());
    }
}

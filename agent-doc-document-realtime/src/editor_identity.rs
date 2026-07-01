//! Pure editor identity helpers.

/// Parse the owning process id from a JetBrains plugin editor id
/// (`jetbrains-<pid>-<uuid>`).
///
/// Returns `None` for non-JetBrains editor ids (for example `vscode-...`) or
/// malformed ids. Callers that own process probing can decide how to handle
/// those ids.
pub fn jetbrains_editor_id_pid(editor_id: &str) -> Option<u32> {
    let rest = editor_id.strip_prefix("jetbrains-")?;
    let pid_str = rest.split('-').next()?;
    if pid_str.is_empty() || !pid_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid_str.parse::<u32>().ok()
}

/// Convert an editor id into a stable patch filename segment.
pub fn sanitize_editor_id_for_filename(editor_id: &str) -> String {
    let sanitized: String = editor_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "editor".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_identity_parses_jetbrains_pid() {
        assert_eq!(jetbrains_editor_id_pid("jetbrains-4242-uuid"), Some(4242));
        assert_eq!(jetbrains_editor_id_pid("jetbrains-0-uuid"), Some(0));
    }

    #[test]
    fn editor_identity_ignores_non_jetbrains_and_malformed_pid() {
        assert_eq!(jetbrains_editor_id_pid("vscode-4242"), None);
        assert_eq!(jetbrains_editor_id_pid("editor-A"), None);
        assert_eq!(jetbrains_editor_id_pid("jetbrains--uuid"), None);
        assert_eq!(jetbrains_editor_id_pid("jetbrains-notapid-uuid"), None);
        assert_eq!(jetbrains_editor_id_pid("jetbrains-42x-uuid"), None);
    }

    #[test]
    fn editor_identity_ignores_pid_overflow() {
        assert_eq!(jetbrains_editor_id_pid("jetbrains-4294967296-uuid"), None);
    }

    #[test]
    fn editor_identity_sanitizes_patch_filename_segments() {
        assert_eq!(
            sanitize_editor_id_for_filename("editor-A_1.2"),
            "editor-A_1.2"
        );
        assert_eq!(
            sanitize_editor_id_for_filename("jetbrains:42/path uuid"),
            "jetbrains_42_path_uuid"
        );
        assert_eq!(sanitize_editor_id_for_filename(""), "editor");
    }
}

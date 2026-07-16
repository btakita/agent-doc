use std::path::Path;

pub fn target_payload_to_editor(
    file: &Path,
    payload: &mut serde_json::Value,
    transport: &str,
    editor_id: &str,
) {
    payload["editor_id"] = serde_json::Value::String(editor_id.to_string());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_payload_targeted file={} transport={} editor_id={}",
            file.display(),
            transport,
            editor_id
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ipc_payload_targets_explicit_registered_editor() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("session.md");
        let mut payload = serde_json::json!({});
        target_payload_to_editor(&doc, &mut payload, "test", "jetbrains-test-owner");
        assert_eq!(
            payload.get("editor_id").and_then(|value| value.as_str()),
            Some("jetbrains-test-owner")
        );
    }
}

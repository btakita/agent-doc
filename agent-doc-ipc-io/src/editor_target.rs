use std::path::Path;

pub fn target_payload_to_editor(
    file: &Path,
    payload: &mut serde_json::Value,
    transport: &str,
    editor_id: &str,
    editor_pid: u64,
) {
    payload["editor_id"] = serde_json::Value::String(editor_id.to_string());
    payload["editor_pid"] = serde_json::Value::Number(editor_pid.into());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_payload_targeted file={} transport={} editor_id={} editor_pid={}",
            file.display(),
            transport,
            editor_id,
            editor_pid
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
        target_payload_to_editor(&doc, &mut payload, "test", "jetbrains-test-owner", 4242);
        assert_eq!(
            payload.get("editor_id").and_then(|value| value.as_str()),
            Some("jetbrains-test-owner")
        );
        assert_eq!(
            payload.get("editor_pid").and_then(|value| value.as_u64()),
            Some(4242)
        );
    }
}

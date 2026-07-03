use std::path::Path;

pub fn live_editor_delivery_target(file: &Path) -> Option<String> {
    let mut file_keys = Vec::new();
    if let Ok(canonical) = file.canonicalize() {
        file_keys.push(canonical.to_string_lossy().to_string());
    }
    let raw = file.to_string_lossy().to_string();
    if !file_keys.iter().any(|key| key == &raw) {
        file_keys.push(raw);
    }

    let owner_candidate = file_keys
        .iter()
        .find_map(|file_key| agent_doc_plugin_owner::live_plugin_owner_consumer_id(file_key))
        .map(
            |editor_id| agent_doc_document_realtime::LiveEditorDeliveryCandidate {
                editor_id: Some(editor_id),
                timestamp_ms: u128::MAX,
                is_live: true,
                has_operator_text_authority: true,
            },
        );
    let live_buffer_candidates = file_keys
        .iter()
        .flat_map(|file_key| agent_doc_debounce::live_buffer_snapshots(file_key))
        .map(|snapshot| {
            let is_live = agent_doc_debounce::live_buffer_snapshot_editor_is_live(&snapshot);
            let has_operator_text_authority =
                snapshot.has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY);
            agent_doc_document_realtime::LiveEditorDeliveryCandidate {
                editor_id: snapshot.editor_id,
                timestamp_ms: snapshot.timestamp_ms,
                is_live,
                has_operator_text_authority,
            }
        });

    agent_doc_document_realtime::select_live_editor_delivery_target(
        owner_candidate.into_iter().chain(live_buffer_candidates),
    )
}

pub fn live_editor_delivery_has_operator_authority(file: &Path) -> bool {
    let mut file_keys = Vec::new();
    if let Ok(canonical) = file.canonicalize() {
        file_keys.push(canonical.to_string_lossy().to_string());
    }
    let raw = file.to_string_lossy().to_string();
    if !file_keys.iter().any(|key| key == &raw) {
        file_keys.push(raw);
    }

    if file_keys
        .iter()
        .any(|file_key| agent_doc_plugin_owner::live_plugin_owner_consumer_id(file_key).is_some())
    {
        return true;
    }

    file_keys.iter().any(|file_key| {
        agent_doc_debounce::live_buffer_snapshots(file_key)
            .into_iter()
            .any(|snapshot| {
                agent_doc_debounce::live_buffer_snapshot_editor_is_live(&snapshot)
                    && snapshot
                        .has_capability(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY)
            })
    })
}

pub fn target_payload_to_live_editor(
    file: &Path,
    payload: &mut serde_json::Value,
    transport: &str,
) -> Option<String> {
    let editor_id = live_editor_delivery_target(file)?;
    payload["editor_id"] = serde_json::Value::String(editor_id.clone());
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "ipc_payload_targeted file={} transport={} editor_id={}",
            file.display(),
            transport,
            editor_id
        ),
    );
    Some(editor_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ipc_payload_targets_live_editor_sidecar_owner() {
        let tmp = TempDir::new().unwrap();
        for subdir in ["live-buffer", "logs"] {
            fs::create_dir_all(tmp.path().join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = tmp.path().join("session.md");
        fs::write(&doc, "saved").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            "saved",
            "jetbrains-test-owner",
            "jetbrains",
            "0.2.197",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let mut payload = serde_json::json!({});
        let target = target_payload_to_live_editor(&doc, &mut payload, "test");

        assert_eq!(target.as_deref(), Some("jetbrains-test-owner"));
        assert_eq!(
            payload.get("editor_id").and_then(|value| value.as_str()),
            Some("jetbrains-test-owner")
        );
    }

    #[test]
    fn ipc_payload_prefers_live_plugin_owner_over_newer_sidecar() {
        let tmp = TempDir::new().unwrap();
        for subdir in ["live-buffer", "plugin-owner", "logs"] {
            fs::create_dir_all(tmp.path().join(".agent-doc").join(subdir)).unwrap();
        }
        let doc = tmp.path().join("session.md");
        fs::write(&doc, "saved").unwrap();
        let doc_str = doc.to_string_lossy().to_string();

        assert!(agent_doc_plugin_owner::try_acquire_plugin_owner(
            &doc_str,
            "jetbrains-owner",
            std::process::id(),
        ));
        agent_doc_debounce::record_live_buffer_digest_content_for_editor_with_capabilities(
            &doc_str,
            "saved plus newer non-owner buffer",
            "jetbrains-newer-nonowner",
            "jetbrains",
            "0.2.197",
            &[agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY],
        )
        .unwrap();

        let mut payload = serde_json::json!({});
        let target = target_payload_to_live_editor(&doc, &mut payload, "test");

        assert_eq!(target.as_deref(), Some("jetbrains-owner"));
        assert_eq!(
            payload.get("editor_id").and_then(|value| value.as_str()),
            Some("jetbrains-owner")
        );
    }
}

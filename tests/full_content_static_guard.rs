use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_source(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|err| {
        panic!("failed to read {path}: {err}");
    })
}

fn assert_only_allowlisted_full_content_refs(path: &str, allowed_fragments: &[&str]) {
    let source = read_source(path);
    assert_only_allowlisted_full_content_refs_in_source(path, &source, allowed_fragments);
}

fn assert_only_allowlisted_full_content_refs_before(
    path: &str,
    stop_marker: &str,
    allowed_fragments: &[&str],
) {
    let source = read_source(path);
    let production_source = source
        .split_once(stop_marker)
        .map(|(before, _)| before)
        .unwrap_or(&source);
    assert_only_allowlisted_full_content_refs_in_source(path, production_source, allowed_fragments);
}

fn assert_only_allowlisted_full_content_refs_in_source(
    path: &str,
    source: &str,
    allowed_fragments: &[&str],
) {
    let mut unexpected = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        if !line.contains("fullContent") {
            continue;
        }
        if allowed_fragments
            .iter()
            .any(|fragment| line.contains(fragment))
        {
            continue;
        }
        unexpected.push(format!("{}:{}: {}", path, line_idx + 1, line.trim()));
    }

    assert!(
        unexpected.is_empty(),
        "fullContent must stay parser/diagnostic-only in production receivers:\n{}",
        unexpected.join("\n")
    );
}

fn assert_source_not_contains(path: &str, needle: &str) {
    let source = read_source(path);
    assert!(
        !source.contains(needle),
        "{path} must not contain `{needle}`"
    );
}

fn assert_source_contains(path: &str, needle: &str) {
    let source = read_source(path);
    assert!(source.contains(needle), "{path} must contain `{needle}`");
}

#[test]
fn vscode_run_agent_doc_uses_jetbrains_route_contract() {
    let vscode = "editors/vscode/src/extension.ts";
    assert_source_contains(vscode, "buildRunRouteCommandArgs");
    assert_source_contains(vscode, "'--dispatch-only'");
    assert_source_contains(vscode, "'--plain-trigger'");
    assert_source_contains(vscode, "'--debounce'");
    assert_source_contains(vscode, "'0'");
    assert_source_contains(vscode, "'--wait-for-ready'");
    assert_source_contains(vscode, "ROUTE_WAIT_FOR_READY_SECONDS = '120'");
    assert_source_contains(vscode, "collectVisibleMarkdownColumns(cwd)");
    assert_source_contains(vscode, "'--col'");
    assert_source_contains(vscode, "'--focus'");
}

#[test]
fn vscode_manifest_exposes_jetbrains_parity_commands() {
    let package_json = "editors/vscode/package.json";
    assert_source_contains(package_json, "\"version\": \"0.2.32\"");
    assert_source_contains(package_json, "\"command\": \"agentDoc.fixDocument\"");
    assert_source_contains(package_json, "\"command\": \"agentDoc.loadTmuxWindow\"");
    assert_source_contains(
        package_json,
        "\"command\": \"agentDoc.interruptClearSessionContext\"",
    );

    let popup = "editors/vscode/src/popupMenu.ts";
    assert_source_contains(popup, "'fixDocument'");
    assert_source_contains(popup, "'loadTmuxWindow'");
    assert_source_contains(popup, "'interruptClear'");
}

fn assert_guard_before_sink(path: &str, anchor: &str, guard: &str, sink: &str) {
    let source = read_source(path);
    let anchor_idx = source
        .find(anchor)
        .unwrap_or_else(|| panic!("{path} missing anchor `{anchor}`"));
    let guarded_region = &source[anchor_idx..];
    let guard_idx = guarded_region
        .find(guard)
        .unwrap_or_else(|| panic!("{path} missing guard `{guard}` after `{anchor}`"));
    let sink_idx = guarded_region
        .find(sink)
        .unwrap_or_else(|| panic!("{path} missing sink `{sink}` after `{anchor}`"));

    assert!(
        guard_idx < sink_idx,
        "{path}: `{guard}` must appear before visible write sink `{sink}` after `{anchor}`"
    );
}

#[test]
fn production_receivers_only_allow_parser_or_diagnostic_full_content_refs() {
    assert_only_allowlisted_full_content_refs_before(
        "agent-doc-orchestration/src/write.rs",
        "#[cfg(test)]",
        &[".get(\"fullContent\")"],
    );
    assert_only_allowlisted_full_content_refs(
        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt",
        &[
            "if (!patch.fullContent.isNullOrEmpty())",
            "val fullContent: String?,",
            "disabled fullContent payloads",
            "fullContentExpectedBufferMatchesUtil",
            "val fullContent = root.get(\"fullContent\")?.asString",
            "fullContent,",
        ],
    );
    assert_only_allowlisted_full_content_refs(
        "editors/vscode/src/extension.ts",
        &[
            "fullContent?: string;",
            "if ((patch.fullContent ?? '') !== '')",
            "if (patch.fullContent != null && patch.fullContent !== '')",
        ],
    );
    assert_only_allowlisted_full_content_refs(
        "editors/vscode/src/patchPlan.ts",
        &[
            "fullContent?: string;",
            "if ((patch.fullContent ?? '') !== '')",
        ],
    );
}

#[test]
fn full_content_values_are_not_reintroduced_into_visible_write_apis() {
    let forbidden = [
        ("agent-doc-orchestration/src/write.rs", "\"fullContent\":"),
        (
            "agent-doc-orchestration/src/write.rs",
            "payload[\"fullContent\"]",
        ),
        (
            "agent-doc-orchestration/src/write.rs",
            "socket_payload[\"fullContent\"]",
        ),
        (
            "agent-doc-orchestration/src/write.rs",
            "ipc_payload[\"fullContent\"]",
        ),
        (
            "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt",
            "document.setText(patch.fullContent)",
        ),
        (
            "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt",
            "setBinaryContent(patch.fullContent",
        ),
        (
            "editors/vscode/src/extension.ts",
            "fullRange, patch.fullContent",
        ),
        (
            "editors/vscode/src/extension.ts",
            "edit.replace(fileUri, fullRange, patch.fullContent)",
        ),
    ];

    for (path, needle) in forbidden {
        assert_source_not_contains(path, needle);
    }
}

#[test]
fn receiver_full_content_rejections_precede_visible_write_sinks() {
    let jetbrains = "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt";
    assert_guard_before_sink(
        jetbrains,
        "private fun handleSocketMessageV2",
        "if (!patch.fullContent.isNullOrEmpty())",
        "awaitIdleBeforeDocumentMutation(patch.file, \"socket patch\")",
    );
    assert_guard_before_sink(
        jetbrains,
        "private fun processPatchFile",
        "if (!patch.fullContent.isNullOrEmpty())",
        "awaitIdleBeforeDocumentMutation(patch.file, \"file patch\")",
    );
    assert_guard_before_sink(
        jetbrains,
        "private fun applyPatch(patch: IpcPatch): Boolean",
        "if (!patch.fullContent.isNullOrEmpty())",
        "document.setText(result)",
    );
    assert_guard_before_sink(
        jetbrains,
        "private fun applyPatchViaVfs",
        "if (!patch.fullContent.isNullOrEmpty())",
        "targetFile.setBinaryContent(result.toByteArray",
    );

    let vscode = "editors/vscode/src/extension.ts";
    assert_guard_before_sink(
        vscode,
        "private async onPatchFileCreated",
        "if ((patch.fullContent ?? '') !== '')",
        "awaitIdleBeforeDocumentMutation(patch.file, 'file patch'",
    );
    assert_guard_before_sink(
        vscode,
        "private async applyPatch",
        "if (patch.fullContent != null && patch.fullContent !== '')",
        "edit.replace(fileUri, fullRange, content)",
    );
}

#[test]
fn node_patches_apply_before_legacy_visible_write_sinks() {
    let jetbrains = "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt";
    assert_guard_before_sink(
        jetbrains,
        "private fun applyPatch(patch: IpcPatch): Boolean",
        "NativePatching.applyNodePatches",
        "document.setText(result)",
    );
    assert_guard_before_sink(
        jetbrains,
        "private fun applyPatchViaVfs",
        "NativePatching.applyNodePatches",
        "targetFile.setBinaryContent(result.toByteArray",
    );
    assert_source_contains(
        jetbrains,
        "skipping legacy component patch for node-patched component",
    );
    assert_source_contains(
        jetbrains,
        "skipping legacy VFS component patch for node-patched component",
    );

    let vscode = "editors/vscode/src/extension.ts";
    assert_guard_before_sink(
        vscode,
        "private async applyPatch",
        "native.applyNodePatches",
        "edit.replace(fileUri, fullRange, content)",
    );
    assert_source_contains(vscode, "skipping legacy component patch for node-patched");
}

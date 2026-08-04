//! Shared binary/plugin vocabulary for Lazily document authority.
//!
//! These names are transport capabilities, not alternate state stores. The
//! Rust core and editor plugins use the same tokens at the Lazily seam so the
//! implementation cannot silently drift into a second live-buffer model.

/// The editor supplies a complete visible-value observation through Lazily
/// current state; operator mutation provenance is carried separately.
pub const OPERATOR_TEXT_AUTHORITY_CAPABILITY: &str = "operator_text_authority_v1";

/// The editor reports Lazily delivery receipts for visible-write proof.
pub const LAZILY_TRANSPORT_RECEIPTS_CAPABILITY: &str = "lazily_transport_receipts_v1";

/// The editor originates ordinary local mutations as bounded causal splices.
///
/// Full document values remain bootstrap/recovery observations and may not be
/// promoted to operator mutations by diffing them against an adapter shadow.
pub const BOUNDED_EDITOR_SPLICES_CAPABILITY: &str = "bounded_editor_splices_v1";

/// The editor consumes the shared typed intent vocabulary over its PID-scoped
/// endpoint. Adapters advertise this only after every intent they accept has the
/// same fail-closed and receipt behavior as the Rust contract.
pub const TYPED_EDITOR_INTENTS_CAPABILITY: &str = "typed_editor_intents_v1";

/// The editor and core can exchange the lossless semantic cell tree.
pub const LOSSLESS_TREE_CRDT_CAPABILITY: &str = "lossless_tree_crdt_v1";

/// `#ctrlkillreregister` Tier 3 — the editor asks `agent_doc_peer_replicas_missing`
/// about itself on startup and on reconnect, and rebuilds whatever it names.
///
/// This is a **retirement condition**, not a feature flag. A peer advertising it
/// repairs itself from replicated state, so the controller's Tier 1 restart fan-out
/// must skip it: that push exists only for plugins predating the pull, and every
/// push is a delivery that can fail to reach its endpoint (`reload-lib reached 1/4
/// endpoints`). Retiring per-peer off the converged registration set means neither
/// side needs to be upgraded first and there is no flag day.
pub const PEER_REPLICA_PULL_CAPABILITY: &str = "peer_replica_pull_v1";

/// The adapter exposes its production CRDT forwarder, controller transport, and
/// native FFI node through the headless cross-editor conformance harness.
pub const CROSS_EDITOR_NATIVE_HARNESS_CAPABILITY: &str = "cross_editor_native_harness_v1";

/// The adapter recovers on its own when the controller socket has no listener.
///
/// `#rebootselfheal`. A host reboot leaves the socket in one of two states that
/// both mean "nothing is listening" — the file vanished with the tmpfs
/// (`ENOENT`), or it outlived the process that bound it (`ECONNREFUSED`) — and
/// neither is fixable by retrying the connect. An adapter that surfaces either
/// verbatim stays broken until a human deletes the socket by hand, which is what
/// happened on 2026-08-03 after an X11 wedge and reboot.
///
/// Recovery must **delegate**: the binary already adopts a live controller,
/// unlinks a stale socket file, and launches. An adapter that reimplements any
/// of that is how the editor and the binary drift into disagreeing about
/// controller liveness. It applies only to operator-initiated lanes — the
/// passive editor-surface observation lane must still never launch a controller.
pub const CONTROLLER_REBOOT_SELF_HEAL_CAPABILITY: &str = "controller_reboot_self_heal_v1";

/// Required capabilities for a plugin that participates in live authority.
pub const REQUIRED_LAZILY_EDITOR_CAPABILITIES: [&str; 2] = [
    OPERATOR_TEXT_AUTHORITY_CAPABILITY,
    LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
];

pub fn has_capability(capabilities: &[String], required: &str) -> bool {
    capabilities.iter().any(|capability| capability == required)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLUGIN_PARITY: &str = include_str!("../../editors/plugin-parity.tsv");
    const JETBRAINS_SOURCES: &[&str] = &[
        include_str!(
            "../../editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/TypingTracker.kt"
        ),
        include_str!(
            "../../editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PatchWatcher.kt"
        ),
        include_str!(
            "../../editors/jetbrains/src/test/kotlin/com/github/btakita/agentdoc/CrossEditorHarnessMain.kt"
        ),
        include_str!(
            "../../editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CpRouteClient.kt"
        ),
        include_str!(
            "../../editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
        ),
        include_str!(
            "../../editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CrdtReplicaManager.kt"
        ),
    ];
    const VSCODE_SOURCES: &[&str] = &[
        include_str!("../../editors/vscode/src/native.ts"),
        include_str!("../../editors/vscode/src/editorIntent.ts"),
        include_str!("../../editors/vscode/src/crossEditorHarness.ts"),
        include_str!("../../editors/vscode/src/extension.ts"),
        include_str!("../../editors/vscode/src/crdtReplica.ts"),
    ];
    const ZED_SOURCES: &[&str] = &[include_str!("../../editors/zed/src/agent_doc.rs")];

    #[derive(Debug)]
    struct FeatureParity<'a> {
        feature: &'a str,
        core: &'a str,
        jetbrains: &'a str,
        vscode: &'a str,
        zed: &'a str,
    }

    fn parity_rows() -> Vec<FeatureParity<'static>> {
        PLUGIN_PARITY
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let columns = line.split('\t').collect::<Vec<_>>();
                assert_eq!(columns.len(), 5, "invalid plugin parity row: {line}");
                FeatureParity {
                    feature: columns[0],
                    core: columns[1],
                    jetbrains: columns[2],
                    vscode: columns[3],
                    zed: columns[4],
                }
            })
            .collect()
    }

    fn adapter_sources(adapter: &str) -> &'static [&'static str] {
        match adapter {
            "jetbrains" => JETBRAINS_SOURCES,
            "vscode" => VSCODE_SOURCES,
            "zed" => ZED_SOURCES,
            other => panic!("unknown editor adapter {other}"),
        }
    }

    #[test]
    fn required_capabilities_use_the_lazily_contract_vocabulary() {
        assert_eq!(
            REQUIRED_LAZILY_EDITOR_CAPABILITIES,
            ["operator_text_authority_v1", "lazily_transport_receipts_v1"]
        );
    }

    #[test]
    fn supported_surface_adapters_bridge_visible_membership_into_the_reactive_graph() {
        let jetbrains_listener = JETBRAINS_SOURCES
            .iter()
            .find(|source| source.contains("class EditorTabSyncListener"))
            .expect("JetBrains editor-surface source");
        let vscode_extension = VSCODE_SOURCES
            .iter()
            .find(|source| source.contains("function requestSurfaceObservation"))
            .expect("VS Code editor-surface source");

        assert!(
            jetbrains_listener
                .contains("override fun fileOpened(source: FileEditorManager, file: VirtualFile)"),
            "JetBrains must observe visible membership that completes after startup layout seeding",
        );
        assert!(
            jetbrains_listener.contains("onEditorLayoutChanged(source.project)"),
            "JetBrains file-open events must invalidate the shared editor-surface source",
        );
        assert!(
            vscode_extension.contains("vscode.window.onDidChangeVisibleTextEditors"),
            "VS Code must observe visible membership changes",
        );
        assert!(
            JETBRAINS_SOURCES
                .iter()
                .any(|source| source.contains("editor_surface_observe"))
                && VSCODE_SOURCES
                    .iter()
                    .any(|source| source.contains("editor_surface_observe")),
            "supported adapters must feed the controller-owned editor-surface graph",
        );
    }

    #[test]
    fn bounded_editor_splice_peers_preserve_host_causality() {
        let jetbrains_manager = JETBRAINS_SOURCES
            .iter()
            .find(|source| source.contains("class CrdtReplicaManager"))
            .expect("JetBrains CRDT manager source");
        let vscode_manager = VSCODE_SOURCES
            .iter()
            .find(|source| source.contains("class CrdtReplicaManager"))
            .expect("VS Code CRDT manager source");
        let vscode_extension = VSCODE_SOURCES
            .iter()
            .find(|source| source.contains("e.contentChanges.map"))
            .expect("VS Code document-event source");

        assert!(
            jetbrains_manager.contains("offsetUtf16 = event.offset")
                && jetbrains_manager.contains("oldFragment = oldFragment")
                && jetbrains_manager.contains("newFragment = newFragment")
                && jetbrains_manager
                    .contains("prepareLocalEditorEditsUtil(beforeText, currentEdits)")
                && jetbrains_manager.contains("isDocumentUnsaved(event.document)"),
            "JetBrains must publish exact dirty DocumentEvent splices",
        );
        let jetbrains_forward = jetbrains_manager
            .split_once("private fun forwardLocalEditsFromShadow(")
            .expect("JetBrains local splice forwarder")
            .1
            .split_once("fun requestRemoteDrain(")
            .expect("JetBrains local splice forwarder boundary")
            .0;
        assert!(
            !jetbrains_forward.contains("tryReadDocumentText(document)")
                && !jetbrains_forward.contains("coalescedLocalEditUtil(beforeText, editorText)"),
            "JetBrains ordinary local mutation must not widen to a whole-buffer diff",
        );
        assert!(
            vscode_extension.contains("rangeOffset: change.rangeOffset")
                && vscode_extension.contains("rangeLength: change.rangeLength")
                && vscode_manager.contains("applyReplicaTextChange(oldText, change)")
                && vscode_manager.contains("advanceNonOperatorProjectionEpoch(filePath)"),
            "VS Code must preserve bounded contentChanges and fence non-operator epochs",
        );
    }

    #[test]
    fn plugin_feature_conformance_selects_only_supported_peers() {
        let rows = parity_rows();
        let known_features = [
            OPERATOR_TEXT_AUTHORITY_CAPABILITY,
            LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
            BOUNDED_EDITOR_SPLICES_CAPABILITY,
            TYPED_EDITOR_INTENTS_CAPABILITY,
            LOSSLESS_TREE_CRDT_CAPABILITY,
            PEER_REPLICA_PULL_CAPABILITY,
            "native_hot_reload_generation_v1",
            CROSS_EDITOR_NATIVE_HARNESS_CAPABILITY,
            CONTROLLER_REBOOT_SELF_HEAL_CAPABILITY,
        ];
        assert_eq!(
            rows.iter().map(|row| row.feature).collect::<Vec<_>>(),
            known_features,
            "the parity matrix must cover the shared Rust capability vocabulary in contract order",
        );

        for row in rows {
            let adapters = [
                ("jetbrains", row.jetbrains),
                ("vscode", row.vscode),
                ("zed", row.zed),
            ];
            if row.core == "required" {
                assert_eq!(
                    row.jetbrains, "supported",
                    "JetBrains must implement required capability {}",
                    row.feature,
                );
                assert_eq!(
                    row.vscode, "supported",
                    "VS Code must implement required capability {}",
                    row.feature,
                );
            } else {
                assert_eq!(
                    row.core, "optional",
                    "invalid core state for {}",
                    row.feature
                );
            }

            let selected = adapters
                .iter()
                .filter(|(_, state)| matches!(*state, "supported" | "conditional"))
                .map(|(adapter, _)| *adapter)
                .collect::<Vec<_>>();
            assert!(
                !selected.is_empty()
                    || row.core == "optional"
                        && adapters.iter().all(|(_, state)| *state == "staged"),
                "feature {} needs a conformance peer unless every adapter is explicitly staged",
                row.feature,
            );

            for (adapter, state) in adapters {
                assert!(
                    matches!(state, "supported" | "conditional" | "staged"),
                    "invalid {adapter} state {state} for {}",
                    row.feature,
                );
                let advertised = adapter_sources(adapter)
                    .iter()
                    .any(|source| source.contains(row.feature));
                match state {
                    "supported" | "conditional" => assert!(
                        advertised,
                        "{adapter} is selected for {} conformance but does not advertise it",
                        row.feature,
                    ),
                    "staged" => assert!(
                        !advertised,
                        "{adapter} advertises staged capability {} before parity is complete",
                        row.feature,
                    ),
                    _ => unreachable!(),
                }
            }
        }
    }
}

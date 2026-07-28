use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(workspace_root().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn dependency_names(manifest: &str) -> Vec<&str> {
    let mut in_dependencies = false;
    manifest
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_dependencies = trimmed == "[dependencies]";
                return None;
            }
            if !in_dependencies || trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            trimmed.split_once('=').map(|(name, _)| name.trim())
        })
        .collect()
}

#[test]
fn core_policy_owners_are_focused_and_legacy_paths_stay_deleted() {
    let root = workspace_root();
    for forbidden in [
        "agent-doc-core/Cargo.toml",
        "agent-doc-core/src/lib.rs",
        "agent-doc-core/src/component.rs",
        "agent-doc-core/src/pending.rs",
        "agent-doc-core/src/queue_item_lifecycle.rs",
        "agent-doc-core/src/template.rs",
        "agent-doc-core/src/crdt_sync.rs",
    ] {
        assert!(
            !root.join(forbidden).exists(),
            "legacy core path must stay deleted: {forbidden}"
        );
    }

    let focused_owners = [
        ("agent-doc-element/src/element.rs", "pub fn parse("),
        (
            "agent-doc-element-backlog/src/backlog.rs",
            "pub struct PendingItem",
        ),
        (
            "agent-doc-element-queue/src/lib.rs",
            "pub enum QueueItemLifecycle",
        ),
        (
            "agent-doc-template/src/template.rs",
            "pub struct PatchBlock",
        ),
        (
            "agent-doc-merge/src/crdt_sync.rs",
            "pub struct ReplicaState",
        ),
        (
            "agent-doc-ffi/src/node_patch.rs",
            "pub unsafe extern \"C\" fn agent_doc_apply_node_patches",
        ),
        (
            "agent-doc-ffi/src/lossless_tree.rs",
            "pub unsafe extern \"C\" fn agent_doc_lossless_tree_project",
        ),
        ("agent-doc-sync/src/lib.rs", "pub enum SyncLockDecision"),
    ];
    for (path, symbol) in focused_owners {
        assert!(
            read(path).contains(symbol),
            "{path} must remain the focused owner of `{symbol}`"
        );
    }
}

#[test]
fn pure_owner_crates_do_not_depend_on_effect_layers() {
    for crate_name in [
        "agent-doc-element",
        "agent-doc-element-backlog",
        "agent-doc-element-queue",
        "agent-doc-template",
        "agent-doc-document",
        "agent-doc-merge",
        "agent-doc-sync",
        "agent-doc-ffi",
    ] {
        let manifest = read(&format!("{crate_name}/Cargo.toml"));
        for dependency in dependency_names(&manifest) {
            assert_ne!(
                dependency, "agent-doc-core",
                "{crate_name} must not recreate the deleted core facade"
            );
            assert_ne!(
                dependency, "agent-doc-orchestration",
                "{crate_name} must not depend on orchestration"
            );
            assert!(
                !dependency.ends_with("-io"),
                "{crate_name} pure policy must not depend on effect crate {dependency}"
            );
        }
    }
}

#[test]
fn root_cdylib_is_an_abi_adapter_not_a_second_pure_policy_owner() {
    let root_ffi = read("src/ffi.rs");
    for forbidden in [
        "pub enum SyncLockDecision",
        "pub fn sync_lock_acquire_decision(",
        "struct IpcNodePatchJson",
        "fn parse_node_patch_op(",
        "pub unsafe extern \"C\" fn agent_doc_apply_node_patches(",
        "pub unsafe extern \"C\" fn agent_doc_lossless_tree_project(",
    ] {
        assert!(
            !root_ffi.contains(forbidden),
            "root FFI must delegate focused policy instead of re-owning `{forbidden}`"
        );
    }
    for linked_symbol in [
        "agent_doc_apply_node_patches",
        "agent_doc_lossless_tree_capability",
        "agent_doc_lossless_tree_project",
        "agent_doc_lossless_tree_render",
        "agent_doc_lossless_tree_projection_current",
    ] {
        assert!(
            root_ffi.contains(linked_symbol),
            "root cdylib must force-link focused ABI symbol {linked_symbol}"
        );
    }
    assert!(
        !read("src/lib.rs").contains("ffi_lossless_tree"),
        "root lib must not expose a duplicate lossless-tree FFI module"
    );

    let repair_pending = read("agent-doc-repair-io/src/pending.rs");
    assert!(
        !repair_pending.contains("struct PendingItem"),
        "repair IO may persist pending responses but must not re-own tracked-work semantics"
    );
}

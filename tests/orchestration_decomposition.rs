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

fn rust_sources_under(path: &Path, sources: &mut Vec<PathBuf>) {
    if path.file_name().is_some_and(|name| name == "target") {
        return;
    }
    for entry in fs::read_dir(path)
        .unwrap_or_else(|error| panic!("read directory {}: {error}", path.display()))
    {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            rust_sources_under(&path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
}

#[test]
fn legacy_orchestration_package_and_facade_stay_deleted() {
    let root = workspace_root();
    for forbidden in [
        "agent-doc-orchestration/Cargo.toml",
        "agent-doc-orchestration/build.rs",
        "agent-doc-orchestration/src/lib.rs",
    ] {
        assert!(
            !root.join(forbidden).exists(),
            "the retired orchestration package must not restore {forbidden}"
        );
    }

    let root_manifest = read("Cargo.toml");
    assert!(
        !root_manifest
            .lines()
            .any(|line| line.trim() == "\"agent-doc-orchestration\","),
        "the retired orchestration crate must not rejoin the workspace"
    );

    for entry in fs::read_dir(&root).unwrap() {
        let entry = entry.unwrap();
        let manifest_path = entry.path().join("Cargo.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = fs::read_to_string(&manifest_path).unwrap();
        assert!(
            !dependency_names(&manifest).contains(&"agent-doc-orchestration"),
            "{} must import its focused owner instead of recreating an orchestration facade",
            manifest_path.display()
        );
    }

    let mut sources = Vec::new();
    rust_sources_under(&root.join("src"), &mut sources);
    for entry in fs::read_dir(&root).unwrap() {
        let entry = entry.unwrap();
        let source_root = entry.path().join("src");
        if source_root.is_dir() {
            rust_sources_under(&source_root, &mut sources);
        }
    }
    for source_path in sources {
        let source = fs::read_to_string(&source_path).unwrap();
        assert!(
            !source.contains("agent_doc_orchestration::"),
            "{} must call the focused owner directly",
            source_path.display()
        );
    }
}

#[test]
fn orchestration_responsibilities_have_focused_owners() {
    let focused_owners = [
        (
            "agent-doc-turn/src/lib.rs",
            "pub struct TurnLifecycleMachine",
        ),
        (
            "agent-doc-document-realtime/src/lib.rs",
            "pub enum DocumentRealtimeState",
        ),
        (
            "agent-doc-supervisor/src/lib.rs",
            "pub enum SupervisorState",
        ),
        (
            "agent-doc-controller/src/lib.rs",
            "pub struct ActorBindingDecision",
        ),
        (
            "agent-doc-editor-surface/src/lib.rs",
            "pub struct EditorSurface",
        ),
        ("agent-doc-tmux/src/lib.rs", "pub enum TmuxPaneActivity"),
        (
            "agent-doc-closeout-runtime-io/src/lib.rs",
            "pub fn session_check",
        ),
        (
            "agent-doc-repair-runtime-io/src/lib.rs",
            "pub fn repair_coordinator_effects",
        ),
        ("agent-doc-route-io/src/lib.rs", "pub mod runtime_effects"),
    ];

    for (path, ownership_token) in focused_owners {
        assert!(
            read(path).contains(ownership_token),
            "{path} must remain the focused owner identified by `{ownership_token}`"
        );
    }
}

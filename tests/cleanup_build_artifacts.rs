use std::fs;
use std::process::Command;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = []\n",
    )
    .unwrap();
    for path in [
        "target/debug",
        "editors/jetbrains/build/distributions",
        ".agent-doc-build-worker",
        ".cargo/registry",
        ".tsift/artifacts",
        "target-cache",
    ] {
        fs::create_dir_all(root.path().join(path)).unwrap();
    }
    root
}

#[test]
fn release_cleanup_removes_only_repo_owned_build_outputs() {
    let root = fixture();
    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/cleanup-build-artifacts.sh"
        ))
        .args(["--root", root.path().to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());

    for removed in [
        "target",
        "editors/jetbrains/build",
        ".agent-doc-build-worker",
    ] {
        assert!(
            !root.path().join(removed).exists(),
            "{removed} was retained"
        );
    }
    for preserved in [".cargo/registry", ".tsift/artifacts", "target-cache"] {
        assert!(
            root.path().join(preserved).exists(),
            "{preserved} was removed"
        );
    }
}

#[test]
fn release_cleanup_can_be_disabled_for_incremental_build_caching() {
    let root = fixture();
    let status = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/cleanup-build-artifacts.sh"
        ))
        .args(["--root", root.path().to_str().unwrap()])
        .env("AGENT_DOC_CLEAN_BUILD_ARTIFACTS", "0")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(root.path().join("target").exists());
    assert!(root.path().join("editors/jetbrains/build").exists());
}
